import test from "node:test";
import assert from "node:assert/strict";
import {attachRemoteVoice, microphoneCapabilityError, microphoneError, syncRemoteVoice} from "./remote-voice.js";

const flush = () => new Promise(resolve => setImmediate(resolve));
const deferred = () => {let resolve, reject;const promise = new Promise((done, fail) => {resolve = done;reject = fail;});return {promise, resolve, reject};};
function stream() {
  const listeners = new Map();
  const track = {readyState: "live", enabled: true, stop() {this.readyState = "ended";}, addEventListener(name, listener) {listeners.set(name, listener);}, end() {this.readyState = "ended";listeners.get("ended")?.();}};
  return {track, getTracks: () => [track], getAudioTracks: () => [track]};
}
function media() {
  return {audioEpoch: 0, session: {voice: {stage: "active", id: "call", local_muted: false}}, changes: [], failures: [],
    changed(extra) {this.changes.push(extra);}, voiceFailed(error, id) {this.failures.push({error, id});}};
}
function environment(t, value) {
  const original = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  Object.defineProperty(globalThis, "navigator", {configurable: true, value});
  t.after(() => original ? Object.defineProperty(globalThis, "navigator", original) : delete globalThis.navigator);
}

test("unsupported capture distinguishes insecure origins and missing WKWebView capability", () => {
  assert.equal(microphoneCapabilityError({isSecureContext: false}).code, "insecure_context");
  const mac = microphoneCapabilityError({window: {__TAURI__: {}}, navigator: {platform: "MacIntel"}});
  assert.equal(mac.code, "capture_unavailable");
  assert.match(mac.message, /macOS/);
  assert.doesNotMatch(mac.message, /undefined/);
  assert.equal(microphoneCapabilityError({navigator: {mediaDevices: {getUserMedia() {}}}}), null);
  assert.match(microphoneError({name: "NotAllowedError"}).message, /权限/);
});

test("missing mediaDevices reports only a scoped voice failure", async t => {
  environment(t, {});
  const state = media();
  await syncRemoteVoice(state);
  assert.equal(state.failures.length, 1);
  assert.equal(state.failures[0].id, "call");
  assert.equal(state.failures[0].error.code, "capture_unavailable");
  assert.equal(state.closed, undefined);
});

test("duplicate active updates share a pending consent request", async t => {
  const consent = deferred(), captured = stream();let requests = 0, attached = 0;
  environment(t, {mediaDevices: {getUserMedia() {requests++;return consent.promise;}}});
  const state = media();state.voice = {sender: {async replaceTrack(track) {assert.equal(track, captured.track);attached++;}}};
  const first = syncRemoteVoice(state), second = syncRemoteVoice(state);
  consent.resolve(captured);await Promise.all([first, second]);
  assert.equal(requests, 1);assert.equal(attached, 1);assert.equal(state.microphone, captured);
});

test("an old call's late permission result cannot replace or stop a redialled microphone", async t => {
  const first = deferred(), second = deferred(), oldStream = stream(), newStream = stream();let requests = 0;
  environment(t, {mediaDevices: {getUserMedia() {return ++requests === 1 ? first.promise : second.promise;}}});
  const state = media(), attached = [];state.voice = {sender: {async replaceTrack(track) {attached.push(track);}}};
  const oldCall = syncRemoteVoice(state);
  state.session.voice = {stage: "active", id: "redial", local_muted: false};
  const newCall = syncRemoteVoice(state);second.resolve(newStream);await newCall;
  first.resolve(oldStream);await oldCall;
  assert.equal(oldStream.track.readyState, "ended");assert.equal(newStream.track.readyState, "live");
  assert.deepEqual(attached, [newStream.track]);assert.equal(state.microphone, newStream);
});

test("a failed microphone switch keeps the existing call and reports the selected device failure", async t => {
  const working = stream();let requests = 0;
  environment(t, {mediaDevices: {async getUserMedia() {if (++requests === 1) return working;throw Object.assign(new Error("gone"), {name: "OverconstrainedError"});}}});
  const state = media();await syncRemoteVoice(state);
  await syncRemoteVoice(state, "removed-device");
  assert.equal(state.microphone, working);assert.equal(working.track.readyState, "live");
  assert.equal(state.failures.length, 0);assert.match(state.changes.at(-1).voiceError, /所选麦克风/);
});

test("a redial observed without an idle poll immediately stops the previous call's microphone", async t => {
  const previous = stream(), next = stream(), consent = deferred();let requests = 0;
  environment(t, {mediaDevices: {getUserMedia() {return ++requests === 1 ? Promise.resolve(previous) : consent.promise;}}});
  const state = media();await syncRemoteVoice(state);
  state.session.voice = {stage: "active", id: "next-call", local_muted: false};
  const pending = syncRemoteVoice(state);
  assert.equal(previous.track.readyState, "ended");assert.equal(state.microphone, null);
  consent.resolve(next);await pending;
  assert.equal(state.microphone, next);assert.equal(next.track.readyState, "live");
});

test("mute during asynchronous attachment is applied before the new microphone can transmit", async t => {
  const captured = stream(), attachment = deferred();
  environment(t, {mediaDevices: {async getUserMedia() {return captured;}}});
  const state = media();state.voice = {sender: {replaceTrack(track) {assert.equal(track.enabled, false);return attachment.promise;}}};
  const pending = syncRemoteVoice(state);await flush();
  state.session.voice.local_muted = true;attachment.resolve();await pending;
  assert.equal(captured.track.enabled, false);assert.equal(state.microphone, captured);
});

test("microphone obtained before the incoming offer attaches to the negotiated audio sender", async t => {
  const captured = stream();environment(t, {mediaDevices: {async getUserMedia() {return captured;}}});
  const state = media();await syncRemoteVoice(state);
  let attached;state.voice = {direction: "sendrecv", sender: {async replaceTrack(track) {attached = track;}}};
  await attachRemoteVoice(state);
  assert.equal(attached, captured.track);assert.equal(state.voice.direction, "sendrecv");
});

test("microphone obtained after audio negotiation attaches without replacing the sendrecv transceiver", async t => {
  const consent = deferred(), captured = stream();environment(t, {mediaDevices: {getUserMedia() {return consent.promise;}}});
  const state = media(), attached = [];const voice = {direction: "sendrecv", sender: {async replaceTrack(track) {attached.push(track);}}};state.voice = voice;
  await attachRemoteVoice(state);const pending = syncRemoteVoice(state);
  consent.resolve(captured);await pending;
  assert.equal(state.voice, voice);assert.equal(voice.direction, "sendrecv");assert.deepEqual(attached, [null, captured.track]);
});

test("a detached old microphone ending cannot hang up the replacement", async t => {
  const oldStream = stream(), nextStream = stream();let requests = 0;
  environment(t, {mediaDevices: {async getUserMedia() {return ++requests === 1 ? oldStream : nextStream;}}});
  const state = media();await syncRemoteVoice(state);await syncRemoteVoice(state, "next-device");
  oldStream.track.end();assert.equal(state.failures.length, 0);
  nextStream.track.end();assert.equal(state.failures.length, 1);assert.equal(state.failures[0].id, "call");
});

test("capture with no audio track stops the returned resources and gives a scoped error", async t => {
  const invalid = stream();invalid.getAudioTracks = () => [];
  environment(t, {mediaDevices: {async getUserMedia() {return invalid;}}});
  const state = media();await syncRemoteVoice(state);
  assert.equal(invalid.track.readyState, "ended");assert.equal(state.failures[0].error.code, "no_audio_track");
});
