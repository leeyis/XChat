function tracePoints(context, points) {
  if (!points.length) return;
  context.beginPath();
  context.moveTo(points[0].x, points[0].y);
  for (const point of points.slice(1)) context.lineTo(point.x, point.y);
}

function drawArrow(context, start, end, size) {
  const angle = Math.atan2(end.y - start.y, end.x - start.x);
  const head = Math.max(12, size * 4);
  context.beginPath();
  context.moveTo(start.x, start.y);
  context.lineTo(end.x, end.y);
  context.moveTo(end.x, end.y);
  context.lineTo(
    end.x - head * Math.cos(angle - Math.PI / 6),
    end.y - head * Math.sin(angle - Math.PI / 6),
  );
  context.moveTo(end.x, end.y);
  context.lineTo(
    end.x - head * Math.cos(angle + Math.PI / 6),
    end.y - head * Math.sin(angle + Math.PI / 6),
  );
  context.stroke();
}

function drawMosaic(context, mosaic, operation, createCanvas) {
  const mask = createCanvas();
  mask.width = context.canvas.width;
  mask.height = context.canvas.height;
  const maskContext = mask.getContext("2d");
  maskContext.strokeStyle = "#fff";
  maskContext.lineWidth = operation.size * 3;
  maskContext.lineCap = "round";
  maskContext.lineJoin = "round";
  tracePoints(maskContext, operation.points);
  maskContext.stroke();

  const layer = createCanvas();
  layer.width = context.canvas.width;
  layer.height = context.canvas.height;
  const layerContext = layer.getContext("2d");
  layerContext.drawImage(mosaic, 0, 0);
  layerContext.globalCompositeOperation = "destination-in";
  layerContext.drawImage(mask, 0, 0);
  context.drawImage(layer, 0, 0);
}

function browserCanvas() {
  return document.createElement("canvas");
}

export function captureEditorActionAvailability({
  conversationId,
  nativeCopy,
  pinEditing = false,
} = {}) {
  return {
    canCopy: Boolean(nativeCopy && !pinEditing),
    canFinish: Boolean(pinEditing || conversationId),
  };
}

export function createCaptureHistory() {
  return { operations: [], undo: [], redo: [] };
}

export function addCaptureOperation(history, operation) {
  return {
    operations: [...history.operations, operation],
    undo: [...history.undo, history.operations],
    redo: [],
  };
}

export function undoCaptureOperation(history) {
  if (!history.undo.length) return history;
  return {
    operations: history.undo.at(-1),
    undo: history.undo.slice(0, -1),
    redo: [...history.redo, history.operations],
  };
}

export function redoCaptureOperation(history) {
  if (!history.redo.length) return history;
  return {
    operations: history.redo.at(-1),
    undo: [...history.undo, history.operations],
    redo: history.redo.slice(0, -1),
  };
}

export function replaceCaptureOperation(history, id, nextOperation) {
  const index = history.operations.findIndex(
    (operation) => operation.id === id,
  );
  if (index < 0) return history;
  const operations = history.operations.slice();
  operations[index] = { ...nextOperation, id };
  return {
    operations,
    undo: [...history.undo, history.operations],
    redo: [],
  };
}

export function removeCaptureOperation(history, id) {
  const operations = history.operations.filter(
    (operation) => operation.id !== id,
  );
  if (operations.length === history.operations.length) return history;
  return {
    operations,
    undo: [...history.undo, history.operations],
    redo: [],
  };
}

export function isCaptureOperationHidden(operation, hiddenTextId) {
  return hiddenTextId != null && operation.id === hiddenTextId;
}

export function placeCaptureToolbar(
  selection,
  toolbar,
  viewport,
  gap = 8,
  padding = 8,
) {
  const below = selection.y + selection.height + gap;
  const side =
    below + toolbar.height <= viewport.height - padding ? "bottom" : "top";
  const desiredTop =
    side === "bottom" ? below : selection.y - gap - toolbar.height;
  const maxLeft = Math.max(padding, viewport.width - toolbar.width - padding);
  const maxTop = Math.max(padding, viewport.height - toolbar.height - padding);
  return {
    left: Math.min(
      maxLeft,
      Math.max(padding, selection.x + selection.width - toolbar.width),
    ),
    top: Math.min(maxTop, Math.max(padding, desiredTop)),
    side,
  };
}

function clamp(value, minimum, maximum) {
  return Math.min(Math.max(value, minimum), maximum);
}

export const CAPTURE_PIN_TOOLBAR_AREA = 72;

export function expandCaptureWindowSize(
  size,
  scaleFactor = 1,
  extraHeight = CAPTURE_PIN_TOOLBAR_AREA,
) {
  const width = Number(size?.width);
  const height = Number(size?.height);
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
    return null;
  }
  const factor = Number(scaleFactor) > 0 ? Number(scaleFactor) : 1;
  return {
    type: "Physical",
    width: Math.round(width),
    height: Math.round(height + extraHeight * factor),
  };
}

export function placeCaptureTextInput(
  point,
  canvas,
  selection,
  desiredWidth = 220,
  editorHeight = 36,
  padding = 6,
) {
  const width = Math.min(
    desiredWidth,
    Math.max(96, selection.width - padding * 2),
  );
  const displayX = (point.x / Math.max(1, canvas.width)) * selection.width;
  const displayY = (point.y / Math.max(1, canvas.height)) * selection.height;
  return {
    left: Math.round(
      clamp(displayX, padding, Math.max(padding, selection.width - width - padding)),
    ),
    top: Math.round(
      clamp(
        displayY,
        padding,
        Math.max(padding, selection.height - editorHeight - padding),
      ),
    ),
    width: Math.round(width),
  };
}

export function placeCaptureTextControls(
  input,
  selectionWidth,
  buttonSize = 28,
  gap = 8,
  inset = 4,
) {
  const width = buttonSize * 2 + gap;
  const inputHeight = Math.max(
    38,
    Number(input.displayFontSize || 16) * 1.25 + 10,
  );
  return {
    left: Math.round(clamp(input.left + input.width - width - inset, 6, Math.max(6, selectionWidth - width - 6))),
    top: Math.round(input.top + (inputHeight - buttonSize) / 2),
    width,
    height: buttonSize,
  };
}

export function fitPinnedCapture(
  width,
  height,
  maxWidth = 960,
  maxHeight = 720,
) {
  if (!(width > 0) || !(height > 0)) return 1;
  return Math.min(1, maxWidth / width, maxHeight / height);
}

export function nextPinnedCaptureZoom(
  current,
  deltaY,
  minimum = 0.2,
  maximum = 3,
) {
  const factor = deltaY < 0 ? 1.1 : 1 / 1.1;
  return Math.round(clamp(current * factor, minimum, maximum) * 100) / 100;
}

export function togglePinnedCaptureToolbar(visible) {
  return !Boolean(visible);
}

export function placePinnedCaptureMenu(
  point,
  menu,
  viewport,
  padding = 8,
) {
  return {
    left: Math.round(
      clamp(point.x, padding, Math.max(padding, viewport.width - menu.width - padding)),
    ),
    top: Math.round(
      clamp(point.y, padding, Math.max(padding, viewport.height - menu.height - padding)),
    ),
  };
}

export function placePinnedCaptureToolbarBelowImage(
  image,
  toolbar,
  viewport,
  gap = 8,
  padding = 8,
) {
  const left = clamp(
    image.x + image.width - toolbar.width,
    padding,
    Math.max(padding, viewport.width - toolbar.width - padding),
  );
  const below = image.y + image.height + gap;
  return {
    left: Math.round(left),
    top: Math.round(below),
    gap,
  };
}

export function normalizeCaptureSelection(start, end, viewport) {
  const startX = clamp(start.x, 0, viewport.width);
  const startY = clamp(start.y, 0, viewport.height);
  const endX = clamp(end.x, 0, viewport.width);
  const endY = clamp(end.y, 0, viewport.height);
  return {
    x: Math.min(startX, endX),
    y: Math.min(startY, endY),
    width: Math.abs(endX - startX),
    height: Math.abs(endY - startY),
  };
}

export function moveCaptureSelection(selection, delta, viewport) {
  return {
    ...selection,
    x: clamp(
      selection.x + delta.x,
      0,
      Math.max(0, viewport.width - selection.width),
    ),
    y: clamp(
      selection.y + delta.y,
      0,
      Math.max(0, viewport.height - selection.height),
    ),
  };
}

export function resizeCaptureSelection(
  selection,
  handle,
  point,
  viewport,
  minimum = 24,
) {
  const right = selection.x + selection.width;
  const bottom = selection.y + selection.height;
  let x = selection.x;
  let y = selection.y;
  let nextRight = right;
  let nextBottom = bottom;

  if (handle.includes("w")) x = clamp(point.x, 0, right - minimum);
  if (handle.includes("e")) {
    nextRight = clamp(point.x, selection.x + minimum, viewport.width);
  }
  if (handle.includes("n")) y = clamp(point.y, 0, bottom - minimum);
  if (handle.includes("s")) {
    nextBottom = clamp(point.y, selection.y + minimum, viewport.height);
  }
  return {
    x,
    y,
    width: nextRight - x,
    height: nextBottom - y,
  };
}

export function drawCaptureOperation(
  context,
  operation,
  mosaic,
  createCanvas = browserCanvas,
) {
  context.save();
  context.strokeStyle = operation.color;
  context.fillStyle = operation.color;
  context.lineWidth = operation.size;
  context.lineCap = "round";
  context.lineJoin = "round";
  if (operation.tool === "pen") {
    tracePoints(context, operation.points);
    context.stroke();
  } else if (operation.tool === "mosaic") {
    drawMosaic(context, mosaic, operation, createCanvas);
  } else if (operation.tool === "rectangle") {
    context.strokeRect(
      operation.start.x,
      operation.start.y,
      operation.end.x - operation.start.x,
      operation.end.y - operation.start.y,
    );
  } else if (operation.tool === "ellipse") {
    const width = operation.end.x - operation.start.x;
    const height = operation.end.y - operation.start.y;
    context.beginPath();
    context.ellipse(
      operation.start.x + width / 2,
      operation.start.y + height / 2,
      Math.abs(width / 2),
      Math.abs(height / 2),
      0,
      0,
      Math.PI * 2,
    );
    context.stroke();
  } else if (operation.tool === "arrow") {
    drawArrow(context, operation.start, operation.end, operation.size);
  } else if (operation.tool === "text") {
    const style = normalizeCaptureTextStyle(operation);
    context.font = `${style.fontStyle} ${style.fontWeight} ${style.fontSize}px ${style.fontFamily}`;
    context.textBaseline = "top";
    context.fillText(operation.text, operation.start.x, operation.start.y);
  }
  context.restore();
}

export const DEFAULT_CAPTURE_TEXT_STYLE = {
  fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", sans-serif",
  fontWeight: "600",
  fontStyle: "normal",
};

export function normalizeCaptureTextStyle(operation = {}) {
  return {
    fontFamily: operation.fontFamily || DEFAULT_CAPTURE_TEXT_STYLE.fontFamily,
    fontWeight: operation.fontWeight || DEFAULT_CAPTURE_TEXT_STYLE.fontWeight,
    fontStyle: operation.fontStyle || DEFAULT_CAPTURE_TEXT_STYLE.fontStyle,
    fontSize: Math.max(16, Number(operation.fontSize || operation.size * 5 || 20)),
  };
}

export function updateCaptureTextStyle(input, style = {}, scale = 1) {
  if (!input) return input;
  const next = { ...input };
  if (style.color !== undefined) next.color = style.color;
  if (style.fontFamily !== undefined) next.fontFamily = style.fontFamily;
  if (style.fontWeight !== undefined) next.fontWeight = style.fontWeight;
  if (style.fontStyle !== undefined) next.fontStyle = style.fontStyle;
  if (style.fontSize !== undefined) {
    next.displayFontSize = Number(style.fontSize);
    next.fontSize = Number(style.fontSize) * Math.max(1, Number(scale) || 1);
  }
  return next;
}

/**
 * Resolve the font-size values used by the canvas operation and the DOM input.
 *
 * Capture operations store their font size in canvas pixels, while the editor
 * controls use CSS/display pixels.  Keeping this conversion in one place is
 * important when an existing annotation is reopened: feeding the canvas value
 * back into the display control and scaling it again on the next edit would
 * make each newly-created input grow on every edit/delete cycle.
 */
export function resolveCaptureTextInputFontSize(
  source = null,
  scale = 1,
  fallbackDisplaySize = 20,
) {
  const safeScale = Number.isFinite(Number(scale)) && Number(scale) > 0
    ? Number(scale)
    : 1;
  const fallback = Math.max(1, Number(fallbackDisplaySize) || 20);
  const storedSize = Number(source?.fontSize || source?.size * 5);
  if (Number.isFinite(storedSize) && storedSize > 0) {
    return {
      fontSize: storedSize,
      displayFontSize: storedSize / safeScale,
    };
  }
  return {
    fontSize: fallback * safeScale,
    displayFontSize: fallback,
  };
}

export function createTextOperation(input, color, size, style = {}) {
  const text = String(input?.value ?? "").trim();
  if (!text) return null;
  return {
    id: input?.id,
    tool: "text",
    start: { x: input.x, y: input.y },
    text,
    color,
    size,
    fontFamily: style.fontFamily || DEFAULT_CAPTURE_TEXT_STYLE.fontFamily,
    fontWeight: style.fontWeight || DEFAULT_CAPTURE_TEXT_STYLE.fontWeight,
    fontStyle: style.fontStyle || DEFAULT_CAPTURE_TEXT_STYLE.fontStyle,
    fontSize: Math.max(16, Number(style.fontSize || size * 5)),
  };
}

export function shouldCancelCaptureTextEdit(original, value) {
  return Boolean(original) && !String(value ?? "").trim();
}

function captureTextBounds(operation, measureText) {
  const style = normalizeCaptureTextStyle(operation);
  const height = style.fontSize;
  const width = Math.max(
    0,
    Number(measureText(operation.text, height, operation)) || 0,
  );
  return {
    x: operation.start.x,
    y: operation.start.y,
    width,
    height,
  };
}

export function hitTestCaptureText(operations, point, measureText) {
  for (let index = operations.length - 1; index >= 0; index -= 1) {
    const operation = operations[index];
    if (operation.tool !== "text") continue;
    const bounds = captureTextBounds(operation, measureText);
    if (
      point.x >= bounds.x &&
      point.x <= bounds.x + bounds.width &&
      point.y >= bounds.y &&
      point.y <= bounds.y + bounds.height
    ) {
      return operation;
    }
  }
  return null;
}

export function moveCaptureTextOperation(
  operation,
  delta,
  canvas,
  measureText,
) {
  const bounds = captureTextBounds(operation, measureText);
  return {
    ...operation,
    start: {
      x: clamp(
        operation.start.x + delta.x,
        0,
        Math.max(0, canvas.width - bounds.width),
      ),
      y: clamp(
        operation.start.y + delta.y,
        0,
        Math.max(0, canvas.height - bounds.height),
      ),
    },
  };
}
