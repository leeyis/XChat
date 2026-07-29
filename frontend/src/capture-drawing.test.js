import assert from "node:assert/strict";
import test from "node:test";
import {
  addCaptureOperation,
  createTextOperation,
  createCaptureHistory,
  drawCaptureOperation,
  moveCaptureSelection,
  normalizeCaptureSelection,
  placeCaptureToolbar,
  redoCaptureOperation,
  resizeCaptureSelection,
  undoCaptureOperation,
} from "./capture-drawing.js";

function fakeContext(label, calls, canvas = { width: 320, height: 200 }) {
  const record = (method) => (...args) => calls.push([label, method, ...args]);
  return {
    canvas,
    save: record("save"),
    restore: record("restore"),
    beginPath: record("beginPath"),
    moveTo: record("moveTo"),
    lineTo: record("lineTo"),
    stroke: record("stroke"),
    strokeRect: record("strokeRect"),
    ellipse: record("ellipse"),
    fillText: record("fillText"),
    drawImage: record("drawImage"),
  };
}

test("all capture annotation tools render through the canvas boundary", () => {
  const calls = [];
  const context = fakeContext("target", calls);
  const canvases = [];
  const createCanvas = () => {
    const canvas = { width: 0, height: 0 };
    const layerContext = fakeContext(`layer-${canvases.length}`, calls, canvas);
    canvas.getContext = () => layerContext;
    canvases.push(canvas);
    return canvas;
  };
  const style = { color: "#f00", size: 4 };

  drawCaptureOperation(context, {
    ...style,
    tool: "rectangle",
    start: { x: 10, y: 20 },
    end: { x: 30, y: 60 },
  });
  drawCaptureOperation(context, {
    ...style,
    tool: "ellipse",
    start: { x: 10, y: 20 },
    end: { x: 30, y: 60 },
  });
  drawCaptureOperation(context, {
    ...style,
    tool: "arrow",
    start: { x: 0, y: 0 },
    end: { x: 40, y: 0 },
  });
  drawCaptureOperation(context, {
    ...style,
    tool: "pen",
    points: [{ x: 1, y: 2 }, { x: 3, y: 4 }, { x: 5, y: 6 }],
  });
  drawCaptureOperation(
    context,
    {
      ...style,
      tool: "mosaic",
      points: [{ x: 2, y: 3 }, { x: 8, y: 9 }],
    },
    { id: "pixelated-source" },
    createCanvas,
  );
  drawCaptureOperation(context, {
    ...style,
    tool: "text",
    start: { x: 7, y: 11 },
    text: "Xchat",
  });

  assert.ok(
    calls.some(
      ([label, method, x, y, width, height]) =>
        label === "target" &&
        method === "strokeRect" &&
        x === 10 &&
        y === 20 &&
        width === 20 &&
        height === 40,
    ),
  );
  assert.ok(
    calls.some(
      ([label, method, x, y, radiusX, radiusY]) =>
        label === "target" &&
        method === "ellipse" &&
        x === 20 &&
        y === 40 &&
        radiusX === 10 &&
        radiusY === 20,
    ),
  );
  assert.equal(
    calls.filter(([label, method]) => label === "target" && method === "lineTo").length,
    5,
  );
  assert.equal(canvases.length, 2);
  assert.ok(
    calls.some(
      ([label, method, layer]) =>
        label === "target" && method === "drawImage" && layer === canvases[1],
    ),
  );
  assert.ok(
    calls.some(
      ([label, method, text, x, y]) =>
        label === "target" &&
        method === "fillText" &&
        text === "Xchat" &&
        x === 7 &&
        y === 11,
    ),
  );
});

test("text annotations commit trimmed content and cancel blank input", () => {
  assert.deepEqual(
    createTextOperation({ x: 12, y: 24, value: "  note  " }, "#0f0", 6),
    {
      tool: "text",
      start: { x: 12, y: 24 },
      text: "note",
      color: "#0f0",
      size: 6,
    },
  );
  assert.equal(
    createTextOperation({ x: 12, y: 24, value: " \n " }, "#0f0", 6),
    null,
  );
});

test("capture history supports undo, redo, and clears redo after a new annotation", () => {
  const rectangle = { tool: "rectangle" };
  const arrow = { tool: "arrow" };
  const text = { tool: "text" };

  let history = createCaptureHistory();
  history = addCaptureOperation(history, rectangle);
  history = addCaptureOperation(history, arrow);
  history = undoCaptureOperation(history);
  assert.deepEqual(history, {
    operations: [rectangle],
    redo: [arrow],
  });

  history = redoCaptureOperation(history);
  assert.deepEqual(history, {
    operations: [rectangle, arrow],
    redo: [],
  });

  history = undoCaptureOperation(history);
  history = addCaptureOperation(history, text);
  assert.deepEqual(history, {
    operations: [rectangle, text],
    redo: [],
  });
});

test("capture toolbar follows the selection, flips above, and stays on screen", () => {
  const toolbar = { width: 360, height: 48 };
  const viewport = { width: 1000, height: 700 };

  assert.deepEqual(
    placeCaptureToolbar(
      { x: 400, y: 200, width: 300, height: 200 },
      toolbar,
      viewport,
    ),
    { left: 340, top: 408, side: "bottom" },
  );
  assert.deepEqual(
    placeCaptureToolbar(
      { x: 400, y: 650, width: 300, height: 30 },
      toolbar,
      viewport,
    ),
    { left: 340, top: 594, side: "top" },
  );
  assert.deepEqual(
    placeCaptureToolbar(
      { x: 10, y: 200, width: 60, height: 200 },
      toolbar,
      viewport,
    ),
    { left: 8, top: 408, side: "bottom" },
  );
});

test("capture selection normalizes, moves, and resizes inside the viewport", () => {
  const viewport = { width: 1000, height: 700 };

  assert.deepEqual(
    normalizeCaptureSelection(
      { x: -20, y: 80 },
      { x: 120, y: 10 },
      viewport,
    ),
    { x: 0, y: 10, width: 120, height: 70 },
  );
  assert.deepEqual(
    moveCaptureSelection(
      { x: 700, y: 500, width: 400, height: 300 },
      { x: 100, y: 100 },
      viewport,
    ),
    { x: 600, y: 400, width: 400, height: 300 },
  );
  assert.deepEqual(
    resizeCaptureSelection(
      { x: 400, y: 300, width: 300, height: 200 },
      "nw",
      { x: 350, y: 250 },
      viewport,
    ),
    { x: 350, y: 250, width: 350, height: 250 },
  );
  assert.deepEqual(
    resizeCaptureSelection(
      { x: 400, y: 300, width: 300, height: 200 },
      "w",
      { x: 690, y: 300 },
      viewport,
    ),
    { x: 676, y: 300, width: 24, height: 200 },
  );
});
