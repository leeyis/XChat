import test from "node:test";
import assert from "node:assert/strict";
import { captureView, capturePoint, captureDisplayRect, captureCrop, captureSizeOptions, captureTextEditorPlacement, stepCaptureSize, moveCaptureAnchor, normalizePinView } from "./capture-model.js";

test("capture coordinates round trip under mixed scaling and letterboxing", () => {
  const image = { width: 2880, height: 2000 }, anchor = { x: 712.5, y: 843.75, width: 600, height: 90 };
  for (const viewport of [{ width: 1440, height: 1000 }, { width: 900, height: 700 }, { width: 1100, height: 760 }]) {
    const view = captureView(image, viewport), screen = captureDisplayRect(anchor, view), point = capturePoint(screen, view);
    assert.ok(Math.abs(point.x - anchor.x) < .0001);
    assert.ok(Math.abs(point.y - anchor.y) < .0001);
    assert.equal(screen.width / anchor.width, screen.height / anchor.height);
  }
});
test("text dragging preserves the grab offset instead of using the input border position", () => {
  const anchor = { x: 200, y: 180 }, start = { x: 250, y: 176 }, now = { x: 330, y: 202 };
  const next = moveCaptureAnchor(anchor, start, now, { width: 1440, height: 900 }, { width: 400, height: 75 });
  assert.deepEqual(next, { x: 280, y: 206 });
  assert.deepEqual(anchor, { x: 200, y: 180 });
});
test("edge text remains editable without moving the stored anchor at any display scale", () => {
  const anchor = { x: 1900, y: 970 };
  const layout = { size: 28, lineHeight: 37.8, width: 420, height: 113.4 };
  for (const factor of [1, 1.25, 1.5, 2]) {
    const viewport = { width: 2000 / factor, height: 1000 / factor };
    const view = captureView({ width: 2000, height: 1000 }, viewport);
    const result = captureTextEditorPlacement(anchor, layout, view, viewport);
    assert.equal(result.relocated, true);
    assert.ok(result.left >= 14 && result.top >= 14);
    assert.ok(result.left + result.width <= viewport.width - 14 + .001);
    assert.ok(result.top + result.height <= viewport.height - 14 + .001);
    assert.deepEqual(anchor, { x: 1900, y: 970 });
  }
});
test("capture exports round and clamp their crop without changing annotation coordinates", () => {
  assert.deepEqual(captureCrop({ x: 98.8, y: 58.8, width: 10, height: 20 }, { width: 100, height: 60 }), { x: 99, y: 59, width: 1, height: 1 });
  assert.deepEqual(captureCrop({ x: -.5, y: -3, width: 640.4, height: 480.6 }, { width: 1440, height: 1000 }), { x: 0, y: 0, width: 640, height: 481 });
});
test("wheel parameters respect each tool's range and physical pixel step", () => {
  for (const tool of ["rectangle", "ellipse", "line", "polyline", "arrow", "pen", "marker", "mosaic", "blur", "eraser", "text"]) {
    const { min, max, step } = captureSizeOptions(tool);
    assert.equal(stepCaptureSize(tool, min, -1), min);
    assert.equal(stepCaptureSize(tool, max, 1), max);
    assert.equal(stepCaptureSize(tool, min, 1), min + step);
  }
});
test("saved pin preferences keep shadow opt-out and safely normalize old records", () => {
  assert.equal(normalizePinView({}).shadow, true);
  const view = normalizePinView({ shadow: false, rotation: -90, scale: 100, opacity: 0, hidden: true, through: true });
  assert.equal(view.shadow, false);
  assert.equal(view.rotation, 270);
  assert.equal(view.scale, 8);
  assert.equal(view.opacity, .15);
  assert.equal(view.hidden, true);
  assert.equal(view.through, true);
});
