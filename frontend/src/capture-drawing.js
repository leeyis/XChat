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
