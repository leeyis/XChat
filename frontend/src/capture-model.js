// Source image pixels are the only persistent coordinate space.
export const CAPTURE_TOOLS = [
  ["select", "调整选区", "Selection"], ["rectangle", "矩形", "Rectangle"],
  ["ellipse", "椭圆", "Ellipse"], ["line", "直线", "Line"],
  ["polyline", "折线", "Polyline"], ["arrow", "箭头", "Arrow"],
  ["pen", "画笔", "Pen"], ["marker", "荧光笔", "Highlighter"],
  ["text", "文字", "Text"], ["mosaic", "马赛克", "Mosaic"],
  ["blur", "模糊", "Blur"], ["eraser", "橡皮擦", "Eraser"],
];
export const CAPTURE_COLORS = ["#ea5455", "#efad35", "#18ac71", "#2c87ca", "#263830", "#ffffff"];
export const CAPTURE_FONTS = [
  ['"Microsoft YaHei", "PingFang SC", sans-serif', "微软雅黑 / 苹方", "System"],
  ['"SimSun", "Songti SC", serif', "宋体", "Serif"],
  ['Consolas, "SFMono-Regular", monospace', "等宽", "Monospace"],
];
export const DEFAULT_TOOL_SIZES = { rectangle: 2, ellipse: 2, line: 2, polyline: 2, arrow: 3, pen: 3, marker: 18, text: 28, mosaic: 12, blur: 8, eraser: 24 };
export const clampCapture = (value, min, max) => Math.max(min, Math.min(max, value));
export const captureId = () => globalThis.crypto?.randomUUID?.() || `capture-${Date.now()}-${Math.random().toString(36).slice(2)}`;

export function captureView(image, viewport) {
  const scale = Math.min(viewport.width / image.width, viewport.height / image.height);
  return { scale, x: (viewport.width - image.width * scale) / 2, y: (viewport.height - image.height * scale) / 2 };
}
export function capturePoint(point, view) {
  if (view.matrix) {
    const [a, b, c, d, x, y] = view.matrix, determinant = a * d - b * c;
    return { x: (d * (point.x - x) - c * (point.y - y)) / determinant, y: (a * (point.y - y) - b * (point.x - x)) / determinant };
  }
  return { x: (point.x - view.x) / view.scale, y: (point.y - view.y) / view.scale };
}
export function captureDisplayPoint(point, view) {
  if (view.matrix) {
    const [a, b, c, d, x, y] = view.matrix;
    return { x: a * point.x + c * point.y + x, y: b * point.x + d * point.y + y };
  }
  return { x: view.x + point.x * view.scale, y: view.y + point.y * view.scale };
}
export function captureDisplayRect(rect, view) {
  if (view.matrix) {
    const points = [[0, 0], [1, 0], [1, 1], [0, 1]].map(([x, y]) => captureDisplayPoint({ x: rect.x + rect.width * x, y: rect.y + rect.height * y }, view));
    const x = Math.min(...points.map(point => point.x)), y = Math.min(...points.map(point => point.y));
    return { x, y, width: Math.max(...points.map(point => point.x)) - x, height: Math.max(...points.map(point => point.y)) - y };
  }
  return { x: view.x + rect.x * view.scale, y: view.y + rect.y * view.scale, width: rect.width * view.scale, height: rect.height * view.scale };
}
// Keep an existing pin anchored when its annotation tools open on another surface.
// The document can contain a crop of a much larger, original screenshot.
export function pinnedCaptureView(region, suppliedView, bounds) {
  const pin = normalizePinView(suppliedView), ratio = Number(bounds.pixelRatio) || 1;
  const swap = pin.rotation % 180 === 90, width = swap ? region.height : region.width, height = swap ? region.width : region.height;
  const scale = pin.thumbnail ? Math.min(160 / width, 120 / height, pin.scale / ratio) : pin.scale / ratio;
  const [cos, sin] = [[1, 0], [0, 1], [-1, 0], [0, -1]][pin.rotation / 90];
  const a = cos * scale * pin.flipX, b = sin * scale * pin.flipX, c = -sin * scale * pin.flipY, d = cos * scale * pin.flipY;
  const centerX = region.x + region.width / 2, centerY = region.y + region.height / 2;
  const x = bounds.x + width * scale / 2 - a * centerX - c * centerY;
  const y = bounds.y + height * scale / 2 - b * centerX - d * centerY;
  return { scale, x, y, matrix: [a, b, c, d, x, y] };
}
export function captureSelectionPinView(region, image, view, origin = { x: 0, y: 0 }, pixelRatio = 1, group = "默认") {
  const display = captureDisplayRect(captureCrop(region, image), view);
  return normalizePinView({ x: Math.round(origin.x + display.x * pixelRatio), y: Math.round(origin.y + display.y * pixelRatio), scale: view.scale * pixelRatio, group });
}
export function captureTextEditorPlacement(anchor, layout, view, viewport) {
  const margin = 14, scale = view.scale;
  if (view.matrix) {
    const [a, b, c, d] = view.matrix, swap = Math.abs(b) > Math.abs(a);
    const sourceWidth = Math.min(Math.max(layout.size * 5, layout.width + 8 / scale), Math.max(1, (swap ? viewport.height : viewport.width) - margin * 2) / scale);
    const sourceHeight = Math.min(Math.max(layout.lineHeight, layout.height), Math.max(layout.lineHeight, ((swap ? viewport.width : viewport.height) - margin * 2) / scale));
    const bounds = captureDisplayRect({ ...anchor, width: sourceWidth, height: sourceHeight }, view), point = captureDisplayPoint(anchor, view);
    const left = clampCapture(bounds.x, margin, Math.max(margin, viewport.width - bounds.width - margin));
    const top = clampCapture(bounds.y, margin, Math.max(margin, viewport.height - bounds.height - margin));
    return { left, top, width: bounds.width, height: bounds.height, sourceWidth, sourceHeight, transform: `matrix(${a},${b},${c},${d},${point.x - bounds.x},${point.y - bounds.y})`, relocated: Math.abs(left - bounds.x) > 1 || Math.abs(top - bounds.y) > 1 };
  }
  const width = Math.min(Math.max(layout.size * 5, layout.width + 8 / scale), Math.max(1, viewport.width - margin * 2) / scale);
  const height = Math.min(Math.max(layout.lineHeight, layout.height), Math.max(layout.lineHeight, (viewport.height - 150) / scale));
  const actualX = view.x + anchor.x * scale, actualY = view.y + anchor.y * scale;
  const left = clampCapture(actualX, margin, Math.max(margin, viewport.width - width * scale - margin));
  const top = clampCapture(actualY, margin, Math.max(margin, viewport.height - height * scale - margin));
  return { left, top, width: width * scale, height: height * scale, relocated: Math.abs(left - actualX) > 1 || Math.abs(top - actualY) > 1 };
}
export function insideCapture(point, rect) {
  return rect && point.x >= rect.x && point.y >= rect.y && point.x <= rect.x + rect.width && point.y <= rect.y + rect.height;
}
export function constrainCapturePoint(start, point, tool, shift) {
  if (!shift) return point;
  const dx = point.x - start.x, dy = point.y - start.y;
  if (tool === "rectangle" || tool === "ellipse") {
    const size = Math.max(Math.abs(dx), Math.abs(dy));
    return { x: start.x + Math.sign(dx || 1) * size, y: start.y + Math.sign(dy || 1) * size };
  }
  const angle = Math.round(Math.atan2(dy, dx) / (Math.PI / 4)) * Math.PI / 4;
  const length = Math.hypot(dx, dy);
  return { x: start.x + Math.cos(angle) * length, y: start.y + Math.sin(angle) * length };
}
export function captureSizeOptions(tool) {
  if (tool === "text") return { min: 10, max: 96, step: 2, zh: "字号", en: "Font size" };
  if (tool === "mosaic") return { min: 4, max: 48, step: 2, zh: "颗粒", en: "Pixel size" };
  if (tool === "blur") return { min: 2, max: 32, step: 1, zh: "模糊", en: "Blur" };
  if (tool === "marker" || tool === "eraser") return { min: 4, max: 80, step: 2, zh: "笔触", en: "Brush size" };
  return { min: 1, max: 32, step: 1, zh: "线宽", en: "Line width" };
}
export function stepCaptureSize(tool, size, direction) {
  const range = captureSizeOptions(tool);
  return clampCapture(size + direction * range.step, range.min, range.max);
}
export function moveCaptureAnchor(anchor, pointerStart, pointerNow, image, bounds) {
  return { x: clampCapture(anchor.x + pointerNow.x - pointerStart.x, 0, Math.max(0, image.width - bounds.width)), y: clampCapture(anchor.y + pointerNow.y - pointerStart.y, 0, Math.max(0, image.height - bounds.height)) };
}
export function captureCrop(region, image) {
  const x = clampCapture(Math.round(region.x), 0, image.width - 1);
  const y = clampCapture(Math.round(region.y), 0, image.height - 1);
  return { x, y, width: clampCapture(Math.round(region.width), 1, image.width - x), height: clampCapture(Math.round(region.height), 1, image.height - y) };
}
export function normalizePinView(view = {}) {
  return { x: Number.isFinite(view.x) ? view.x : 60, y: Number.isFinite(view.y) ? view.y : 80, scale: clampCapture(Number(view.scale) || 1, .1, 8), rotation: ((Math.round((Number(view.rotation) || 0) / 90) * 90) % 360 + 360) % 360, flipX: view.flipX === -1 ? -1 : 1, flipY: view.flipY === -1 ? -1 : 1, opacity: clampCapture(Number.isFinite(view.opacity) ? view.opacity : 1, .15, 1), shadow: view.shadow !== false, hidden: Boolean(view.hidden), through: Boolean(view.through), thumbnail: Boolean(view.thumbnail), group: view.group === "设计参考" ? "设计参考" : "默认" };
}
