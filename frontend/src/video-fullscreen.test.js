import test from "node:test";
import assert from "node:assert/strict";
import { androidVideoOwnsFocus, createVideoFullscreenController } from "./video-fullscreen.js";

function fixture() {
  const doc = new EventTarget();
  const frames = [];
  const stages = [];
  doc.fullscreenElement = null;
  doc.defaultView = { requestAnimationFrame(callback) { frames.push(callback); } };
  doc.querySelector = () => stages.find(stage => stage.dataset.fullscreenPending) || null;
  doc.exitFullscreen = async () => { doc.fullscreenElement = null; doc.dispatchEvent(new Event("fullscreenchange")); };
  const scroller = { isConnected: true, scrollTop: 416, scrollLeft: 9 };
  let focuses = 0;
  const trigger = { focus(options) { assert.equal(options.preventScroll, true); focuses++; } };
  function makeStage() {
    const stage = {
      ownerDocument: doc, dataset: {}, contains: () => false,
      closest(selector) { return selector === ".message-scroll" ? scroller : stage; },
      async requestFullscreen(options) {
        assert.equal(options.navigationUI, "hide");
        doc.fullscreenElement = stage;
        doc.dispatchEvent(new Event("fullscreenchange"));
      },
    };
    stages.push(stage);
    return stage;
  }
  return { doc, scroller, trigger, makeStage, focuses: () => focuses, flush() { while (frames.length) frames.shift()(); } };
}

test("native exit restores chat scroll after Android inset layout without stealing another fullscreen's focus", async () => {
  const f = fixture(), stage = f.makeStage();
  const controller = createVideoFullscreenController(stage, { trigger: f.trigger });
  await controller.enter();
  assert.equal(androidVideoOwnsFocus(f.doc), true);
  f.scroller.scrollTop = 40;
  await f.doc.exitFullscreen(); // Android system back follows this browser event path.
  assert.equal(f.scroller.scrollTop, 416);
  f.scroller.scrollTop = 400; // A later native safe-area update.
  f.flush();
  assert.equal(f.scroller.scrollTop, 416);
  assert.equal(f.scroller.scrollLeft, 9);
  assert.equal(f.focuses(), 1);
  await controller.enter();
  f.doc.fullscreenElement = f.makeStage();
  f.doc.dispatchEvent(new Event("fullscreenchange"));
  controller.destroy();
  assert.ok(f.doc.fullscreenElement);
  assert.equal(f.focuses(), 1);
});

test("rejected fullscreen clears transition focus and allows a later retry", async () => {
  const f = fixture(), stage = f.makeStage(), errors = [];
  const request = stage.requestFullscreen;
  stage.requestFullscreen = async () => { throw new Error("request denied"); };
  const controller = createVideoFullscreenController(stage, { onError: error => errors.push(error.message) });
  await controller.enter();
  assert.deepEqual(errors, ["request denied"]);
  assert.equal(androidVideoOwnsFocus(f.doc), false);
  assert.equal(f.scroller.scrollTop, 416);
  stage.requestFullscreen = request;
  await controller.enter();
  assert.equal(f.doc.fullscreenElement, stage);
  controller.destroy();
});

test("double taps share one pending request and unmount closes a late fullscreen view", async () => {
  const f = fixture(), stage = f.makeStage();
  let complete, requests = 0;
  stage.requestFullscreen = () => {
    requests++;
    return new Promise(resolve => { complete = () => { f.doc.fullscreenElement = stage; resolve(); }; });
  };
  const controller = createVideoFullscreenController(stage);
  const entering = controller.enter();
  await controller.enter();
  assert.equal(requests, 1);
  assert.equal(androidVideoOwnsFocus(f.doc), true);
  controller.destroy();
  complete();
  await entering;
  assert.equal(f.doc.fullscreenElement, null);
  assert.equal(androidVideoOwnsFocus(f.doc), false);
});
