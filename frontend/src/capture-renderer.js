import { CAPTURE_FONTS, captureCrop } from "./capture-model.js";

const baselineCache = new Map();
export function captureCanvas(width, height) {
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(width));
  canvas.height = Math.max(1, Math.round(height));
  return canvas;
}
export async function captureImage(dataUrl) {
  const image = new Image();
  image.src = dataUrl;
  await image.decode();
  return image;
}
export function captureTextLayout(operation, context) {
  const size = operation.fontSize || operation.size || 28;
  const family = operation.fontFamily || CAPTURE_FONTS[0][0];
  const font = `400 ${size}px ${family}`;
  const lineHeight = size * 1.35;
  if (!baselineCache.has(font)) {
    const probe = document.createElement("div");
    Object.assign(probe.style, { position: "fixed", left: "-10000px", top: "0", visibility: "hidden", font, lineHeight: `${lineHeight}px`, whiteSpace: "pre", margin: "0", padding: "0", border: "0" });
    probe.append(document.createTextNode("Hg中文"));
    const marker = document.createElement("i");
    Object.assign(marker.style, { display: "inline-block", width: "0", height: "0", margin: "0", padding: "0", border: "0", verticalAlign: "baseline" });
    probe.append(marker);
    document.body.append(probe);
    baselineCache.set(font, marker.getBoundingClientRect().top - probe.getBoundingClientRect().top);
    probe.remove();
  }
  context.save();
  context.font = font;
  const lines = String(operation.text ?? "").split("\n");
  const width = Math.max(1, ...lines.map(line => context.measureText(line).width));
  context.restore();
  return { font, size, family, lineHeight, baseline: baselineCache.get(font), lines, width, height: Math.max(1, lines.length) * lineHeight };
}
export function captureTextAt(operations, point, context) {
  for (const operation of [...operations].reverse()) {
    if (operation.tool !== "text") continue;
    const rect = captureTextLayout(operation, context);
    if (point.x >= operation.start.x - 3 && point.x <= operation.start.x + rect.width + 3 && point.y >= operation.start.y - 3 && point.y <= operation.start.y + rect.height + 3) return operation;
  }
  return null;
}
function strokePoints(context, points) {
  if (!points?.length) return;
  context.beginPath();
  context.moveTo(points[0].x, points[0].y);
  if (points.length === 1) context.lineTo(points[0].x + .01, points[0].y);
  else points.slice(1).forEach(point => context.lineTo(point.x, point.y));
  context.stroke();
}
export function paintCaptureOperation(context, operation, base) {
  context.save();
  context.strokeStyle = operation.color;
  context.fillStyle = operation.color;
  context.lineWidth = operation.size;
  context.lineCap = "round";
  context.lineJoin = "round";
  const { tool, start, end } = operation;
  if (["pen", "marker", "polyline", "eraser"].includes(tool)) {
    if (tool === "marker") context.globalAlpha = .35;
    if (tool === "eraser") context.globalCompositeOperation = "destination-out";
    strokePoints(context, operation.points);
  } else if (tool === "mosaic" || tool === "blur") {
    // Privacy effects sample the image and previous annotations; erasing the
    // annotation layer later never erases pixels from the underlying image.
    const points = start && end ? [start, end] : operation.points || [];
    if (!points.length) { context.restore(); return; }
    const radius = start && end ? 0 : Math.max(24, operation.size * 3) / 2;
    const padding = tool === "blur" ? operation.size * 3 : operation.size;
    const lowX = Math.min(...points.map(point => point.x)) - radius, lowY = Math.min(...points.map(point => point.y)) - radius;
    const highX = Math.max(...points.map(point => point.x)) + radius, highY = Math.max(...points.map(point => point.y)) + radius;
    const x = Math.max(0, Math.floor((lowX - padding) / operation.size) * operation.size);
    const y = Math.max(0, Math.floor((lowY - padding) / operation.size) * operation.size);
    const width = Math.min(base.width - x, Math.ceil((highX + padding - x) / operation.size) * operation.size);
    const height = Math.min(base.height - y, Math.ceil((highY + padding - y) / operation.size) * operation.size);
    if (width <= 0 || height <= 0) { context.restore(); return; }
    // Work on the affected pixels only. Allocating several full 4K canvases on
    // every pointer event made otherwise small privacy edits noticeably lag.
    const sample = captureCanvas(width, height), sampleContext = sample.getContext("2d");
    sampleContext.drawImage(base, -x, -y);
    sampleContext.drawImage(context.canvas, -x, -y);
    const effect = captureCanvas(width, height), effectContext = effect.getContext("2d");
    if (tool === "mosaic") {
      const small = captureCanvas(Math.ceil(width / operation.size), Math.ceil(height / operation.size));
      small.getContext("2d").drawImage(sample, 0, 0, small.width, small.height);
      effectContext.imageSmoothingEnabled = false;
      effectContext.drawImage(small, 0, 0, width, height);
    } else {
      effectContext.filter = `blur(${operation.size}px)`;
      effectContext.drawImage(sample, 0, 0);
      effectContext.filter = "none";
    }
    const mask = captureCanvas(width, height), maskContext = mask.getContext("2d");
    maskContext.translate(-x, -y);
    if (start && end) {
      maskContext.fillRect(Math.min(start.x, end.x), Math.min(start.y, end.y), Math.abs(end.x - start.x), Math.abs(end.y - start.y));
    } else {
      maskContext.lineWidth = Math.max(24, operation.size * 3);
      maskContext.lineCap = "round";
      maskContext.lineJoin = "round";
      strokePoints(maskContext, operation.points);
    }
    effectContext.globalCompositeOperation = "destination-in";
    effectContext.drawImage(mask, 0, 0);
    context.drawImage(effect, x, y);
  } else if (tool === "rectangle") {
    context.strokeRect(start.x, start.y, end.x - start.x, end.y - start.y);
  } else if (tool === "ellipse") {
    context.beginPath();
    context.ellipse((start.x + end.x) / 2, (start.y + end.y) / 2, Math.abs(end.x - start.x) / 2, Math.abs(end.y - start.y) / 2, 0, 0, Math.PI * 2);
    context.stroke();
  } else if (tool === "line" || tool === "arrow") {
    strokePoints(context, [start, end]);
    if (tool === "arrow") {
      const angle = Math.atan2(end.y - start.y, end.x - start.x), head = Math.max(10, operation.size * 4);
      strokePoints(context, [{ x: end.x - head * Math.cos(angle - Math.PI / 6), y: end.y - head * Math.sin(angle - Math.PI / 6) }, end, { x: end.x - head * Math.cos(angle + Math.PI / 6), y: end.y - head * Math.sin(angle + Math.PI / 6) }]);
    }
  } else if (tool === "text") {
    const layout = captureTextLayout(operation, context);
    context.font = layout.font;
    context.textBaseline = "alphabetic";
    layout.lines.forEach((line, index) => context.fillText(line, start.x, start.y + layout.baseline + index * layout.lineHeight));
  }
  context.restore();
}
export function renderCaptureAnnotations(base, operations, hiddenId = null) {
  const layer = captureCanvas(base.width, base.height);
  const context = layer.getContext("2d");
  operations.forEach(operation => { if (operation.id !== hiddenId) paintCaptureOperation(context, operation, base); });
  return layer;
}
export function exportCapture(base, operations, selection) {
  const region = captureCrop(selection, base);
  const layer = renderCaptureAnnotations(base, operations);
  const output = captureCanvas(region.width, region.height), context = output.getContext("2d");
  context.drawImage(base, region.x, region.y, region.width, region.height, 0, 0, region.width, region.height);
  context.drawImage(layer, region.x, region.y, region.width, region.height, 0, 0, region.width, region.height);
  return output;
}
export async function renderPinnedCapture(record, original = false) {
  const image = await captureImage(record.data_url);
  const view = record.view || {};
  const scale = original ? 1 : view.scale || 1;
  const angle = original ? 0 : (view.rotation || 0) * Math.PI / 180;
  const swap = Math.abs(Math.sin(angle)) > .99;
  const width = (swap ? image.height : image.width) * scale, height = (swap ? image.width : image.height) * scale;
  if (width > 32767 || height > 32767 || width * height > 64000000) throw new Error("当前显示比例的图片过大，请缩小贴图或选择复制原图");
  const canvas = captureCanvas(width, height);
  const context = canvas.getContext("2d");
  context.translate(canvas.width / 2, canvas.height / 2);
  context.rotate(angle);
  context.scale(scale * (original ? 1 : view.flipX || 1), scale * (original ? 1 : view.flipY || 1));
  context.drawImage(image, -image.width / 2, -image.height / 2);
  return canvas;
}
