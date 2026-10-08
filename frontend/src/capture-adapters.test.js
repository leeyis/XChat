import test from "node:test";
import assert from "node:assert/strict";
import { createXChatModule, HttpWsAdapter, TauriAdapter } from "./xchat.js";

function replaceGlobals(values) {
  const descriptors = new Map(Object.keys(values).map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]));
  for (const [key, value] of Object.entries(values)) Object.defineProperty(globalThis, key, { configurable: true, writable: true, value });
  return () => { for (const [key, descriptor] of descriptors) { if (descriptor) Object.defineProperty(globalThis, key, descriptor); else delete globalThis[key]; } };
}

const png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j5ZkAAAAASUVORK5CYII=";

test("web capture remains in the current page and records source dimensions without a conversation", async () => {
  const events = [];
  let stopped = 0, paused = 0, draws = 0;
  const stream = { getTracks: () => [{ stop() { stopped++; } }], getVideoTracks: () => [{ readyState: "live" }] };
  const video = { videoWidth: 2560, videoHeight: 1440, async play() {}, pause() { paused++; } };
  const restore = replaceGlobals({
    navigator: { mediaDevices: { async getDisplayMedia() { return stream; } } },
    document: { createElement: name => name === "video" ? video : { getContext: () => ({ drawImage() { draws++; } }), toBlob(callback) { callback(new Blob(["png"], { type: "image/png" })); } } },
    FileReader: class { readAsDataURL() { this.result = png; queueMicrotask(() => this.onload()); } },
    dispatchEvent(event) { events.push(event); },
    CustomEvent: class { constructor(type, { detail }) { this.type = type; this.detail = detail; } },
    localStorage: { getItem: () => null, setItem() { throw new Error("Captures must not be put into localStorage"); } },
    open() { throw new Error("Capture must not create a popup"); },
  });
  try {
    const web = new HttpWsAdapter();
    const result = await web.startCapture(null);
    assert.equal(result.pending, true);
    assert.equal(events.length, 1);
    assert.equal(events[0].type, "xchat-capture-open");
    assert.deepEqual([events[0].detail.width, events[0].detail.height], [2560, 1440]);
    assert.equal(events[0].detail.conversation_id, null);
    assert.equal(web.pendingCapture().data_url, png);
    assert.equal(stopped, 1);
    assert.equal(paused, 1);
    assert.equal(video.srcObject, null);
    assert.equal(draws, 1);
    web.cancelCapture();
    assert.equal(web.pendingCapture(), null);
  } finally { restore(); }
});

test("failed web capture stops every media track and emits no editor event", async () => {
  let stopped = 0, events = 0;
  const stream = { getTracks: () => [{ stop() { stopped++; } }, { stop() { stopped++; } }] };
  const restore = replaceGlobals({
    navigator: { mediaDevices: { async getDisplayMedia() { return stream; } } },
    document: { createElement: () => ({ async play() { throw new Error("decoder failed"); }, pause() {} }) },
    dispatchEvent() { events++; },
  });
  try {
    const web = new HttpWsAdapter();
    await assert.rejects(web.startCapture("chat-a"), /decoder failed/);
    assert.equal(stopped, 2);
    assert.equal(events, 0);
    assert.equal(web.pendingCapture(), null);
    assert.equal(web.captureInFlight, null);
  } finally { restore(); }
});

test("web draft output uses its captured conversation and rejects unbound imported images", async () => {
  const web = new HttpWsAdapter();
  web.captureSession = { conversation_id: "chat-a", file_name: "capture.png" };
  const first = await web.finishCapture(png);
  assert.equal(first.conversation_id, "chat-a");
  assert.ok(first.file instanceof File);
  assert.equal(first.preview_url, png);
  web.captureSession = { conversation_id: "unrelated-active-chat" };
  const restored = await web.finishCapture(png, { conversationId: "original-chat" });
  assert.equal(restored.conversation_id, "original-chat");
  web.captureSession = { conversation_id: "unrelated-active-chat" };
  await assert.rejects(web.finishCapture(png, { conversationId: null }), error => error.code === "capture_conversation_required");
  assert.equal(web.pendingCapture().conversation_id, "unrelated-active-chat");
});

test("native pin commands carry the stable pin identity and temporary overlay state", async () => {
  const calls = [];
  const native = new TauriAdapter({ core: { async invoke(command, payload) { calls.push([command, payload]); return payload?.view; } } });
  const view = { scale: 1.5, shadow: false, flipX: -1, flipY: 1 };
  await native.pinCapture(png, "pin-a", view, "chat-a");
  await native.updatePinnedCapture("pin-a", view, true);
  await native.copyPinnedCapture(undefined, "pin-a", png);
  await native.savePinnedCapture("pin-b", png);
  await native.closePinnedCapture(false, "pin-b");
  assert.deepEqual(calls, [
    ["pin_capture", { dataUrl: png, pinId: "pin-a", view, conversationId: "chat-a" }],
    ["update_pinned_capture", { pinId: "pin-a", view, overlay: true }],
    ["copy_pinned_capture", { scale: undefined, pinId: "pin-a", dataUrl: png }],
    ["save_pinned_capture", { pinId: "pin-b", dataUrl: png }],
    ["close_pinned_capture", { destroy: false, pinId: "pin-b" }],
  ]);
});

test("main-window imported captures stage drafts without consuming another native editor", async () => {
  const calls = [];
  const native = new TauriAdapter({ core: { async invoke(command, payload) { calls.push([command, payload]); return { file_path: "staged/capture.png", file_name: payload?.fileName }; } } });
  const restore = replaceGlobals({ location: { search: "" } });
  try {
    await native.cancelCapture();
    assert.equal(calls.length, 0);
    const draft = await native.finishCapture(png, { conversationId: "chat-original" });
    assert.equal(calls[0][0], "stage_image_attachment");
    assert.equal(draft.conversation_id, "chat-original");
    assert.equal(draft.file_path, "staged/capture.png");
    await assert.rejects(native.finishCapture(png, { conversationId: null }), error => error.code === "capture_conversation_required");
    assert.equal(calls.length, 1);
  } finally { restore(); }
});

test("same-page capture-ready events are subscribed and removed with the transport", async () => {
  const target = new EventTarget();
  const received = [];
  const restore = replaceGlobals({
    addEventListener: target.addEventListener.bind(target), removeEventListener: target.removeEventListener.bind(target),
    BroadcastChannel: undefined,
  });
  try {
    const native = new TauriAdapter({ core: {}, event: { async listen() { return () => {}; } } });
    const stop = native.subscribe(event => received.push(event));
    const event = new Event("xchat-capture-ready");
    event.detail = { type: "capture-ready", attachment: { id: "capture-id", conversation_id: "chat-a", file_path: "capture.png" } };
    target.dispatchEvent(event);
    assert.deepEqual(received, [{ type: "capture-ready", payload: event.detail.attachment }]);
    stop();
    target.dispatchEvent(event);
    assert.equal(received.length, 1);
  } finally { restore(); }
});


test("a cancelled delayed web capture releases its stream without opening an editor", async () => {
  const web = new HttpWsAdapter();
  let stopped = 0, opened = 0;
  const countdowns = [];
  const stream = { getTracks: () => [{ stop() { stopped++; } }], getVideoTracks: () => [{ readyState: "live" }] };
  const restore = replaceGlobals({
    navigator: { mediaDevices: { async getDisplayMedia() { return stream; } } },
    document: { createElement: () => ({ videoWidth: 640, videoHeight: 480, async play() {}, pause() {} }) },
    localStorage: { getItem: () => JSON.stringify({ delay: 3 }) },
    CustomEvent: class { constructor(type, { detail }) { this.type = type; this.detail = detail; } },
    dispatchEvent(event) {
      if (event.type === "xchat-capture-open") opened++;
      if (event.type === "xchat-capture-countdown") {
        countdowns.push(event.detail);
        if (event.detail.remaining === 3) queueMicrotask(() => web.cancelCaptureStart(event.detail.session_id));
      }
    },
  });
  try {
    await assert.rejects(web.startCapture("chat-a"), error => error.code === "cancelled");
    assert.equal(stopped, 1);
    assert.equal(opened, 0);
    assert.equal(web.pendingCapture(), null);
    assert.equal(countdowns[0].remaining, 3);
    assert.equal(countdowns.at(-1).remaining, 0);
    assert.ok(countdowns[0].session_id);
    assert.equal(web.captureAbort, null);
    assert.equal(web.cancelCaptureStart(countdowns[0].session_id), false);
  } finally { restore(); }
});

test("late output and cancellation cannot clear a newer web capture session", async () => {
  const web = new HttpWsAdapter();
  let finishCopy;
  let markCopyStarted;
  const copyStarted = new Promise(resolve => { markCopyStarted = resolve; });
  const restore = replaceGlobals({
    navigator: { clipboard: { write() { markCopyStarted(); return new Promise(resolve => { finishCopy = resolve; }); } } },
    ClipboardItem: class { constructor(value) { this.value = value; } },
  });
  try {
    web.captureSession = { session_id: "old", conversation_id: "old-chat" };
    const copy = web.copyCapture(png, { sourceSessionId: "old" });
    await copyStarted;
    web.captureSession = { session_id: "new", conversation_id: "new-chat", file_name: "new.png" };
    finishCopy();
    await copy;
    assert.equal(web.pendingCapture().session_id, "new");
    web.cancelCapture("old");
    assert.equal(web.pendingCapture().session_id, "new");
    const oldDraft = await web.finishCapture(png, { sourceSessionId: "old", conversationId: "old-chat" });
    assert.equal(oldDraft.conversation_id, "old-chat");
    assert.notEqual(oldDraft.file_name, "new.png");
    assert.equal(web.pendingCapture().session_id, "new");
    web.pinCapture(png, "imported-pin", {}, null);
    assert.equal(web.pendingCapture().session_id, "new");
    web.cancelCapture("new");
    assert.equal(web.pendingCapture(), null);
  } finally { restore(); }
});

test("native countdown cancellation maps to a quiet cancelled result", async () => {
  const native = new TauriAdapter({ core: { async invoke() { throw "capture_cancelled"; } } });
  await assert.rejects(native.startCapture(null), error => error.code === "cancelled" && error.retryable === false);
});


test("capture preferences patch only the supplied native fields", async () => {
  const calls = [];
  const preferences = { delaySeconds: 5, captureCursor: true, cursorSupported: true };
  const restore = replaceGlobals({ window: { __TAURI__: { core: { async invoke(command, payload) {
    calls.push([command, payload]);
    Object.assign(preferences, payload);
    return { ...preferences };
  } } } } });
  try {
    const workspace = createXChatModule();
    assert.deepEqual((await workspace.dispatch({ type: "capture.preferences", captureCursor: false })).data,
      { delaySeconds: 5, captureCursor: false, cursorSupported: true });
    assert.deepEqual((await workspace.dispatch({ type: "capture.preferences", delaySeconds: 3 })).data,
      { delaySeconds: 3, captureCursor: false, cursorSupported: true });
    assert.equal((await workspace.dispatch({ type: "capture.preferences" })).data.delaySeconds, 3);
    assert.deepEqual(calls, [
      ["set_capture_preferences", { captureCursor: false }],
      ["set_capture_preferences", { delaySeconds: 3 }],
      ["set_capture_preferences", {}],
    ]);
  } finally { restore(); }
});

test("web cursor control follows declared capability and retains omitted preferences", () => {
  const devices = { getSupportedConstraints: () => ({ cursor: true }) };
  const restore = replaceGlobals({ navigator: { mediaDevices: devices },
    localStorage: { getItem: () => JSON.stringify({ delay: 5, captureCursor: true }) } });
  try {
    const web = new HttpWsAdapter();
    assert.deepEqual(web.setCapturePreferences(undefined, false), { delaySeconds: 5, captureCursor: false, cursorSupported: true });
    assert.deepEqual(web.setCapturePreferences(3), { delaySeconds: 3, captureCursor: true, cursorSupported: true });
    devices.getSupportedConstraints = () => ({ displaySurface: true });
    assert.equal(web.setCapturePreferences().cursorSupported, false);
    assert.throws(() => web.setCapturePreferences(undefined, true), error => error.code === "capture_cursor_unsupported");
  } finally { restore(); }
});

async function withCursorCapture(options, check) {
  const observed = { stopped: 0, draws: 0, requests: [], constraints: [] };
  let current = options.current;
  const track = { readyState: "live", kind: "video",
    stop() { observed.stopped++; this.readyState = "ended"; },
    getCapabilities: () => options.modes === undefined ? {} : { cursor: options.modes },
    getSettings: () => current === undefined ? {} : { cursor: current },
    async applyConstraints(constraints) {
      observed.constraints.push(constraints);
      if (options.apply) await options.apply(constraints);
      if (!options.ignoreApplied) current = constraints.cursor.exact;
    },
  };
  const stream = { getVideoTracks: () => [track], getTracks: () => [track] };
  const video = { videoWidth: 640, videoHeight: 480, async play() {}, pause() {} };
  const restore = replaceGlobals({
    navigator: { mediaDevices: {
      getSupportedConstraints: () => options.advertised ? { cursor: true } : { displaySurface: true },
      async getDisplayMedia(value) { observed.requests.push(value); return stream; },
    } },
    localStorage: { getItem: () => JSON.stringify({ delay: 0, captureCursor: options.captureCursor }) },
    document: { createElement: name => name === "video" ? video : {
      getContext: () => ({ drawImage() { observed.draws++; } }),
      toBlob(callback) { callback(new Blob(["png"], { type: "image/png" })); },
    } },
  });
  try { await check(new HttpWsAdapter(), observed); } finally { restore(); }
}

test("supported screen sources receive and verify the requested cursor mode", async () => {
  for (const captureCursor of [true, false]) {
    const desired = captureCursor ? "always" : "never";
    await withCursorCapture({ advertised: true, captureCursor, modes: ["always", "never"], current: desired }, async (web, observed) => {
      const file = await web.capture();
      assert.equal(file.type, "image/png");
      assert.deepEqual(observed.requests, [{ video: { cursor: desired }, audio: false }]);
      assert.equal(web.captureCursorMode, desired);
      assert.equal(observed.draws, 1);
      assert.equal(observed.stopped, 1);
      assert.equal(observed.constraints.length, 0);
    });
  }
});

test("a screen source ignoring the initial cursor hint is corrected and rechecked", async () => {
  await withCursorCapture({ advertised: true, captureCursor: true, modes: ["always", "never"], current: "never" }, async (web, observed) => {
    await web.capture();
    assert.deepEqual(observed.constraints, [{ cursor: { exact: "always" } }]);
    assert.equal(web.captureCursorMode, "always");
    assert.equal(observed.draws, 1);
    assert.equal(observed.stopped, 1);
  });
});

test("unsupported sources and unapplied cursor constraints fail without freezing pixels", async () => {
  for (const options of [
    { advertised: true, captureCursor: true, modes: ["never"], current: "never" },
    { advertised: true, captureCursor: false, modes: ["always"], current: "always" },
    { advertised: true, captureCursor: true, modes: ["always", "never"], current: "never", ignoreApplied: true },
  ]) await withCursorCapture(options, async (web, observed) => {
    await assert.rejects(web.capture(), error => error.code === "capture_cursor_unsupported");
    assert.equal(observed.draws, 0);
    assert.equal(observed.stopped, 1);
  });
});

test("unavailable browser cursor control never blocks default or historical preferences", async () => {
  for (const captureCursor of [false, true]) {
    await withCursorCapture({ advertised: false, captureCursor, modes: ["motion"], current: "motion",
      apply() { throw new Error("Unsupported cursor control must not be invoked"); } }, async (web, observed) => {
      await web.capture();
      assert.deepEqual(observed.requests, [{ video: true, audio: false }]);
      assert.equal(observed.constraints.length, 0);
      assert.equal(web.captureCursorMode, "motion");
      assert.equal(observed.draws, 1);
      assert.equal(observed.stopped, 1);
    });
  }
});

test("cancelling while cursor constraints are applying still releases the source", async () => {
  const controller = new AbortController();
  let started;
  const applying = new Promise(resolve => { started = resolve; });
  await withCursorCapture({ advertised: true, captureCursor: true, modes: ["always", "never"], current: "never",
    apply() { started(); return new Promise(() => {}); } }, async (web, observed) => {
    const capture = web.capture({ signal: controller.signal });
    await applying;
    controller.abort();
    await assert.rejects(capture, error => error.code === "cancelled");
    assert.equal(observed.draws, 0);
    assert.equal(observed.stopped, 1);
  });
});
