import test from "node:test";
import assert from "node:assert/strict";
import { createVoiceRecorder, VOICE_MAX_MS } from "./voice-recorder.js";

function fixture(overrides = {}) {
  let time = 0, tick, stopped = 0, instance;
  class Recorder {
    static isTypeSupported(type) { return type.startsWith("audio/webm"); }
    constructor(stream, options) { this.mimeType = options.mimeType; instance = this; }
    start() { this.state = "recording"; }
    stop() { this.state = "inactive"; queueMicrotask(() => { this.ondataavailable?.({ data: new Blob(["real last chunk"]) }); this.onstop?.(); }); }
  }
  const stream = { getTracks: () => [{ stop() { stopped++; } }] };
  const env = { crypto: { randomUUID: () => "c493690e-7e3d-4514-8c5b-f78a57fa8a16" }, MediaRecorder: Recorder,
    navigator: { mediaDevices: { getUserMedia: async () => stream } }, ...overrides };
  const recorder = createVoiceRecorder({ env, now: () => time, setInterval: fn => { tick = fn; return 1; }, clearInterval: () => { tick = null; } });
  return { recorder, stream, advance(ms) { time += ms; tick?.(); }, stopped: () => stopped, instance: () => instance };
}

test("permission cancelled before it resolves never creates a recorder or leaves a microphone track", async () => {
  let grant;
  const f = fixture({ navigator: { mediaDevices: { getUserMedia: () => new Promise(resolve => { grant = resolve; }) } } });
  const start = f.recorder.start();
  assert.equal(f.recorder.snapshot().phase, "requesting");
  await f.recorder.cancel(); grant(f.stream); await start;
  assert.equal(f.recorder.snapshot().phase, "idle"); assert.equal(f.instance(), undefined); assert.equal(f.stopped(), 1);
});

test("finish waits for the final media chunk and keeps the recording ID stable for retries", async () => {
  const f = fixture(); await f.recorder.start(); f.advance(1300);
  const pending = f.recorder.finish(); assert.equal(f.recorder.snapshot().phase, "stopping");
  const result = await pending;
  assert.equal(await result.blob.text(), "real last chunk"); assert.equal(result.duration_ms, 1300);
  assert.equal(await f.recorder.finish(), result); assert.equal(f.stopped(), 1);
  await f.recorder.cancel(); assert.equal(f.recorder.snapshot().recording, null);
});

test("60 seconds stops into ready without sending, and under one second cannot create a message", async () => {
  const f = fixture(); await f.recorder.start(); f.advance(VOICE_MAX_MS + 250);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(f.recorder.snapshot().phase, "ready"); assert.equal(f.recorder.snapshot().recording.duration_ms, VOICE_MAX_MS);
  await f.recorder.cancel(); await f.recorder.start(); f.advance(500);
  assert.equal(await f.recorder.finish(), null); assert.equal(f.recorder.snapshot().phase, "error");
});

test("late stop events from a cancelled generation cannot overwrite the next recording", async () => {
  const f = fixture(); await f.recorder.start(); const previous = f.instance();
  await f.recorder.cancel(); await f.recorder.start(); previous.onstop();
  assert.equal(f.recorder.snapshot().phase, "recording"); f.recorder.destroy(); assert.ok(f.stopped() >= 2);
});

test("permission and encoding failures release resources and remain retryable", async () => {
  const denied = fixture({ navigator: { mediaDevices: { getUserMedia: async () => { throw Object.assign(new Error(), { name: "NotAllowedError" }); } } } });
  await denied.recorder.start(); assert.match(denied.recorder.snapshot().error, /权限/);
  const f = fixture(); f.instance;
  await f.recorder.start(); f.instance().onerror({ error: new Error("codec failed") });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(f.recorder.snapshot().phase, "error"); assert.equal(f.stopped(), 1);
});
