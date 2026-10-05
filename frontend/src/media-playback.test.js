import test from "node:test";
import assert from "node:assert/strict";
import {
  createMediaPlaybackController,
  detectImageAnimation,
  formatMediaDuration,
  imageAnimationHint,
  imagePrefixIsAnimated,
  mediaPositionKey,
  mediaTransferDirection,
} from "./media-playback.js";

function player(time = 0, duration = 120) {
  return { currentTime: time, duration, ended: false, pauses: 0, pause() { this.pauses += 1; } };
}

test("only the newly started player continues, positions survive conversation remounts", () => {
  const controller = createMediaPlaybackController();
  const audio = player(14), video = player(8);
  const unregisterAudio = controller.register(audio, "a:audio");
  controller.register(video, "a:video");
  controller.play(video);
  assert.equal(audio.pauses, 1);
  assert.equal(video.pauses, 0);
  controller.pauseAll();
  assert.equal(video.pauses, 1);
  unregisterAudio();
  const returnedAudio = player();
  controller.register(returnedAudio, "a:audio");
  controller.restore(returnedAudio);
  assert.equal(returnedAudio.currentTime, 14);
  const otherConversation = player();
  controller.register(otherConversation, "b:audio");
  controller.restore(otherConversation);
  assert.equal(otherConversation.currentTime, 0);
});

test("ended media restarts and stale positions do not seek past the current duration", () => {
  const controller = createMediaPlaybackController();
  const old = player(90);
  const unregister = controller.register(old, "a:video");
  unregister();
  const shorter = player(0, 30);
  controller.register(shorter, "a:video");
  controller.restore(shorter);
  assert.equal(shorter.currentTime, 0);
  shorter.currentTime = 30;
  shorter.ended = true;
  controller.remember(shorter);
  const replay = player();
  controller.register(replay, "a:video");
  controller.restore(replay);
  assert.equal(replay.currentTime, 0);
});

test("metadata-pending remount and StrictMode cleanup retain the saved position", () => {
  const controller = createMediaPlaybackController();
  const old = player(42);
  controller.register(old, "a:audio")();
  const returning = player(0);
  controller.register(returning, "a:audio")();
  controller.register(returning, "a:audio");
  controller.pauseAll();
  controller.remember(returning);
  controller.restore(returning);
  assert.equal(returning.currentTime, 42);
});

test("playback position keys survive numeric id assignment without crossing conversations", () => {
  assert.equal(mediaPositionKey("a", { client_message_id: "stable", id: "pending" }), mediaPositionKey("a", { client_message_id: "stable", id: 12 }));
  assert.notEqual(mediaPositionKey("a", { id: 12 }), mediaPositionKey("b", { id: 12 }));
  assert.equal(formatMediaDuration(NaN), "—");
  assert.equal(formatMediaDuration(61.8), "01:01");
});

test("production send and receive transfer directions keep media progress labels correct", () => {
  assert.equal(mediaTransferDirection({ direction: "send" }, { own: true }), "outgoing");
  assert.equal(mediaTransferDirection({ direction: "receive" }, { own: false }), "incoming");
  assert.equal(mediaTransferDirection({ direction: "upload" }), "outgoing");
  assert.equal(mediaTransferDirection({ direction: "download" }), "incoming");
  assert.equal(mediaTransferDirection({ direction: "unknown" }, { direction: "outgoing" }), "outgoing");
  assert.equal(mediaTransferDirection({}, { own: true }), "outgoing");
  assert.equal(mediaTransferDirection({}, { direction: "receive" }), "incoming");
});

const pngSignature = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]);
function chunk(type, payload = []) {
  const result = new Uint8Array(12 + payload.length);
  new DataView(result.buffer).setUint32(0, payload.length);
  result.set(new TextEncoder().encode(type), 4);
  result.set(payload, 8);
  return result;
}
function concat(...parts) {
  return new Uint8Array(parts.flatMap((part) => [...part]));
}

test("APNG .png and animated WebP are detected while static images remain static", () => {
  assert.equal(imageAnimationHint({ file_name: "image.GIF" }), true);
  assert.equal(imageAnimationHint({ mime_type: "image/apng" }), true);
  assert.equal(imageAnimationHint({ file_name: "motion.png" }), null);
  assert.equal(imagePrefixIsAnimated(concat(pngSignature, chunk("IHDR", [0, 0, 0, 10]), chunk("acTL", [0, 0, 0, 2]), chunk("IDAT"))), true);
  assert.equal(imagePrefixIsAnimated(concat(pngSignature, chunk("IHDR"), chunk("IDAT"))), false);
  const webp = new Uint8Array(30);
  webp.set(new TextEncoder().encode("RIFF"));
  webp.set(new TextEncoder().encode("WEBPVP8X"), 8);
  webp[20] = 0x02;
  assert.equal(imagePrefixIsAnimated(webp), true);
  webp[20] = 0x10;
  assert.equal(imagePrefixIsAnimated(webp), false);
  assert.equal(imagePrefixIsAnimated(new Uint8Array([1, 2])), false);
});

test("animation sniffing reads a bounded prefix even when a server ignores Range", async () => {
  const originalFetch = globalThis.fetch;
  let request, cancelled = false, reads = 0;
  globalThis.fetch = async (url, options) => {
    request = { url, options };
    return { ok: true, body: { getReader: () => ({
      async read() { reads += 1; return { value: new Uint8Array(32768), done: false }; },
      async cancel() { cancelled = true; },
    }) } };
  };
  try {
    assert.equal(await detectImageAnimation({ file_name: "image.webp", url: "/media/12" }), false);
    assert.equal(request.options.headers.Range, "bytes=0-65535");
    assert.equal(reads, 2);
    assert.equal(cancelled, true);
    request = null;
    assert.equal(await detectImageAnimation({ file_name: "image.gif", url: "/media/12" }), true);
    assert.equal(request, null);
  } finally {
    globalThis.fetch = originalFetch;
  }
});
