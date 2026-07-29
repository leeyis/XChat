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

export function createCaptureHistory() {
  return { operations: [], redo: [] };
}

export function addCaptureOperation(history, operation) {
  return {
    operations: [...history.operations, operation],
    redo: [],
  };
}

export function undoCaptureOperation(history) {
  if (!history.operations.length) return history;
  const operations = history.operations.slice(0, -1);
  return {
    operations,
    redo: [...history.redo, history.operations.at(-1)],
  };
}

export function redoCaptureOperation(history) {
  if (!history.redo.length) return history;
  return {
    operations: [...history.operations, history.redo.at(-1)],
    redo: history.redo.slice(0, -1),
  };
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
    context.font = `600 ${Math.max(16, operation.size * 5)}px -apple-system, sans-serif`;
    context.textBaseline = "top";
    context.fillText(operation.text, operation.start.x, operation.start.y);
  }
  context.restore();
}

export function createTextOperation(input, color, size) {
  const text = String(input?.value ?? "").trim();
  if (!text) return null;
  return {
    tool: "text",
    start: { x: input.x, y: input.y },
    text,
    color,
    size,
  };
}
