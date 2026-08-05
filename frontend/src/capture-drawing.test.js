import assert from "node:assert/strict";
import test from "node:test";
import {
  addCaptureOperation,
  CAPTURE_PIN_TOOLBAR_AREA,
  captureEditorActionAvailability,
  createTextOperation,
  createCaptureHistory,
  drawCaptureOperation,
  expandCaptureWindowSize,
  fitPinnedCapture,
  hitTestCaptureText,
  isCaptureOperationHidden,
  moveCaptureSelection,
  moveCaptureTextOperation,
  nextPinnedCaptureZoom,
  normalizeCaptureTextStyle,
  updateCaptureTextStyle,
  togglePinnedCaptureToolbar,
  normalizeCaptureSelection,
  placeCaptureTextInput,
  placeCaptureTextControls,
  placePinnedCaptureMenu,
  placePinnedCaptureToolbarBelowImage,
  placeCaptureToolbar,
  redoCaptureOperation,
  replaceCaptureOperation,
  resizeCaptureSelection,
  resolveCaptureTextInputFontSize,
  shouldCancelCaptureTextEdit,
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
    createTextOperation(
      { id: "text-1", x: 12, y: 24, value: "  note  " },
      "#0f0",
      6,
    ),
    {
      id: "text-1",
      tool: "text",
      start: { x: 12, y: 24 },
      text: "note",
      color: "#0f0",
      size: 6,
      fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
      fontWeight: "600",
      fontStyle: "normal",
      fontSize: 30,
    },
  );
  assert.equal(
    createTextOperation({ x: 12, y: 24, value: " \n " }, "#0f0", 6),
    null,
  );
});

test("text styles normalize old operations and persist explicit font settings", () => {
  assert.deepEqual(normalizeCaptureTextStyle({ size: 4 }), {
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
    fontWeight: "600",
    fontStyle: "normal",
    fontSize: 20,
  });
  assert.deepEqual(
    createTextOperation(
      { x: 1, y: 2, value: "styled" },
      "#fff",
      4,
      { fontFamily: "Arial", fontWeight: "700", fontStyle: "italic", fontSize: 28 },
    ),
    {
      id: undefined,
      tool: "text",
      start: { x: 1, y: 2 },
      text: "styled",
      color: "#fff",
      size: 4,
      fontFamily: "Arial",
      fontWeight: "700",
      fontStyle: "italic",
      fontSize: 28,
    },
  );
});

test("active text input style changes are reflected immediately and preserve canvas scale", () => {
  const input = {
    value: "你好",
    color: "#f00",
    fontFamily: "Arial, sans-serif",
    fontWeight: "600",
    fontStyle: "normal",
    fontSize: 40,
    displayFontSize: 20,
  };
  assert.deepEqual(updateCaptureTextStyle(input, {
    color: "#1687f8",
    fontFamily: "Songti SC, SimSun, serif",
    fontWeight: "700",
    fontStyle: "italic",
    fontSize: 24,
  }, 2), {
    ...input,
    color: "#1687f8",
    fontFamily: "Songti SC, SimSun, serif",
    fontWeight: "700",
    fontStyle: "italic",
    fontSize: 48,
    displayFontSize: 24,
  });
});

test("reopening text annotations keeps display font size stable across edit cycles", () => {
  const scale = 2;
  const reopened = resolveCaptureTextInputFontSize({ fontSize: 40 }, scale, 20);
  assert.deepEqual(reopened, { fontSize: 40, displayFontSize: 20 });

  // A new annotation after deleting the reopened one must use the display
  // value (20px) as its fallback, not the old canvas value (40px).
  const createdAgain = resolveCaptureTextInputFontSize(
    null,
    scale,
    reopened.displayFontSize,
  );
  const createdThirdTime = resolveCaptureTextInputFontSize(
    null,
    scale,
    createdAgain.displayFontSize,
  );
  assert.deepEqual(createdAgain, { fontSize: 40, displayFontSize: 20 });
  assert.deepEqual(createdThirdTime, { fontSize: 40, displayFontSize: 20 });

  // Legacy operations only have the brush-like `size`; they must follow the
  // same conversion and must not poison the next editor cycle.
  const legacy = resolveCaptureTextInputFontSize({ size: 4 }, scale, 20);
  assert.deepEqual(legacy, { fontSize: 20, displayFontSize: 10 });
  assert.deepEqual(
    resolveCaptureTextInputFontSize(null, scale, legacy.displayFontSize),
    { fontSize: 20, displayFontSize: 10 },
  );
});

test("text input controls stay vertically centered for small and large fonts", () => {
  assert.deepEqual(
    placeCaptureTextControls({ left: 100, top: 20, width: 260, displayFontSize: 14 }, 500),
    { left: 292, top: 25, width: 64, height: 28 },
  );
  assert.deepEqual(
    placeCaptureTextControls({ left: 100, top: 20, width: 260, displayFontSize: 40 }, 500),
    { left: 292, top: 36, width: 64, height: 28 },
  );
});

test("capture text input stays visible near selection edges", () => {
  assert.deepEqual(
    placeCaptureTextInput(
      { x: 780, y: 360 },
      { width: 800, height: 400 },
      { width: 420, height: 220 },
    ),
    { left: 194, top: 178, width: 220 },
  );
  assert.deepEqual(
    placeCaptureTextInput(
      { x: 10, y: 8 },
      { width: 800, height: 400 },
      { width: 140, height: 80 },
    ),
    { left: 6, top: 6, width: 128 },
  );
});

test("text annotations can be hit, moved, edited, undone, and redone atomically", () => {
  const first = createTextOperation(
    { id: "text-1", x: 10, y: 10, value: "first" },
    "#f00",
    4,
  );
  const topmost = createTextOperation(
    { id: "text-2", x: 20, y: 20, value: "topmost" },
    "#0f0",
    4,
  );
  const measureText = () => 80;

  assert.equal(
    hitTestCaptureText([first, topmost], { x: 30, y: 30 }, measureText)?.id,
    "text-2",
  );
  assert.equal(
    hitTestCaptureText([first, topmost], { x: 180, y: 90 }, measureText),
    null,
  );

  const moved = moveCaptureTextOperation(
    topmost,
    { x: 1000, y: 1000 },
    { width: 200, height: 100 },
    measureText,
  );
  assert.deepEqual(moved.start, { x: 120, y: 80 });

  let history = addCaptureOperation(createCaptureHistory(), topmost);
  history = replaceCaptureOperation(history, topmost.id, {
    ...moved,
    text: "edited",
  });
  assert.equal(history.operations[0].text, "edited");
  assert.deepEqual(history.operations[0].start, { x: 120, y: 80 });

  history = undoCaptureOperation(history);
  assert.equal(history.operations[0].text, "topmost");
  assert.deepEqual(history.operations[0].start, { x: 20, y: 20 });

  history = redoCaptureOperation(history);
  assert.equal(history.operations[0].text, "edited");
  assert.deepEqual(history.operations[0].start, { x: 120, y: 80 });
});

test("clearing an edited text annotation cancels without deleting it", () => {
  const text = createTextOperation(
    { id: "text-1", x: 10, y: 10, value: "remove me" },
    "#f00",
    4,
  );
  assert.equal(shouldCancelCaptureTextEdit(text, "   "), true);
  assert.equal(shouldCancelCaptureTextEdit(text, "updated"), false);
  assert.equal(shouldCancelCaptureTextEdit(null, "   "), false);
});

test("preview only hides the text operation currently being edited", () => {
  assert.equal(isCaptureOperationHidden({ tool: "mosaic" }, undefined), false);
  assert.equal(
    isCaptureOperationHidden({ id: "text-1", tool: "text" }, "text-1"),
    true,
  );
  assert.equal(
    isCaptureOperationHidden({ id: "text-2", tool: "text" }, "text-1"),
    false,
  );
});

test("pinned capture wheel zoom is bounded and its context menu stays onscreen", () => {
  assert.equal(fitPinnedCapture(1920, 1080), 0.5);
  assert.equal(nextPinnedCaptureZoom(0.5, -120), 0.55);
  assert.equal(nextPinnedCaptureZoom(0.2, 120), 0.2);
  assert.equal(nextPinnedCaptureZoom(3, -120), 3);
  assert.deepEqual(
    placePinnedCaptureMenu(
      { x: 990, y: 690 },
      { width: 260, height: 320 },
      { width: 1000, height: 700 },
    ),
    { left: 732, top: 372 },
  );
});

test("pinned capture toolbar visibility toggles predictably", () => {
  assert.equal(togglePinnedCaptureToolbar(false), true);
  assert.equal(togglePinnedCaptureToolbar(true), false);
});

test("capture editor actions distinguish standalone, Web, and pin editing", () => {
  assert.deepEqual(
    captureEditorActionAvailability({
      conversationId: null,
      nativeCopy: true,
      pinEditing: false,
    }),
    { canCopy: true, canFinish: false },
  );
  assert.deepEqual(
    captureEditorActionAvailability({
      conversationId: "conversation-1",
      nativeCopy: true,
      pinEditing: false,
    }),
    { canCopy: true, canFinish: true },
  );
  assert.deepEqual(
    captureEditorActionAvailability({
      conversationId: "conversation-1",
      nativeCopy: false,
      pinEditing: false,
    }),
    { canCopy: false, canFinish: true },
  );
  assert.deepEqual(
    captureEditorActionAvailability({
      conversationId: null,
      nativeCopy: true,
      pinEditing: true,
    }),
    { canCopy: false, canFinish: true },
  );
});

test("pin editing expands a physical window without changing the image viewport", () => {
  assert.deepEqual(
    expandCaptureWindowSize({ width: 640, height: 720 }, 2),
    { type: "Physical", width: 640, height: 720 + CAPTURE_PIN_TOOLBAR_AREA * 2 },
  );
  assert.equal(expandCaptureWindowSize({ width: 0, height: 720 }, 2), null);
  assert.equal(expandCaptureWindowSize({ width: 640, height: 720 }, 0).height, 720 + CAPTURE_PIN_TOOLBAR_AREA);
});

test("pinned capture toolbar stays in the transparent area below the image", () => {
  assert.deepEqual(
    placePinnedCaptureToolbarBelowImage(
      { x: 100, y: 40, width: 400, height: 200 },
      { width: 360, height: 48 },
      { width: 640, height: 320 },
    ),
    { left: 140, top: 248, gap: 8 },
  );
  const image = { x: 20, y: 0, width: 400, height: 180 };
  const toolbar = { width: 360, height: 48 };
  const placed = placePinnedCaptureToolbarBelowImage(
    image,
    toolbar,
    { width: 640, height: 248 },
  );
  assert.equal(placed.top, image.y + image.height + 8);
  assert.ok(placed.top >= image.y + image.height);
  assert.ok(placed.top + toolbar.height <= 248 - 8);

  const imageViewport = { x: 0, y: 0, width: 640, height: 720 };
  const expandedViewport = {
    width: 640,
    height: 720 + CAPTURE_PIN_TOOLBAR_AREA,
  };
  const normalToolbar = { width: 520, height: 52 };
  const normalPlacement = placePinnedCaptureToolbarBelowImage(
    imageViewport,
    normalToolbar,
    expandedViewport,
  );
  assert.equal(normalPlacement.top, 728);
  assert.ok(normalPlacement.top >= imageViewport.height + 8);
  assert.ok(normalPlacement.top + normalToolbar.height <= expandedViewport.height - 8);
});

test("capture history supports undo, redo, and clears redo after a new annotation", () => {
  const rectangle = { tool: "rectangle" };
  const arrow = { tool: "arrow" };
  const text = { tool: "text" };

  let history = createCaptureHistory();
  history = addCaptureOperation(history, rectangle);
  history = addCaptureOperation(history, arrow);
  history = undoCaptureOperation(history);
  assert.deepEqual(history.operations, [rectangle]);

  history = redoCaptureOperation(history);
  assert.deepEqual(history.operations, [rectangle, arrow]);
  assert.equal(history.redo.length, 0);

  history = undoCaptureOperation(history);
  history = addCaptureOperation(history, text);
  assert.deepEqual(history.operations, [rectangle, text]);
  assert.equal(history.redo.length, 0);
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
