import test from "node:test";
import assert from "node:assert/strict";
import { messageMenuPosition } from "./mobile-menu.js";

test("message menu flips below a top bubble and keeps the arrow aligned", () => {
  const result = messageMenuPosition({ left: 40, right: 240, top: 80, bottom: 126 }, { width: 350, height: 160 }, { left: 12, right: 381, top: 64, bottom: 650 });
  assert.deepEqual(result, { left: 12, top: 138, below: true, arrow: 128 });
});

test("message menu remains visible above the keyboard and near a right edge", () => {
  const result = messageMenuPosition({ left: 330, right: 390, top: 290, bottom: 340 }, { width: 350, height: 160 }, { left: 12, right: 381, top: 64, bottom: 300 });
  assert.deepEqual(result, { left: 31, top: 118, below: false, arrow: 329 });
});
