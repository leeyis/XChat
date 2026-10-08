import { captureCanvas } from "./capture-renderer.js";

// Keep clipboard color detection exact, so an ordinary note containing a color
// remains text. Opaque RGB channels may use bytes or percentages.
export function parseCaptureColor(value) {
  if (typeof value !== "string") return null;
  const text = value.trim();
  if (/^#[\da-f]{3}$/i.test(text)) return "#" + [...text.slice(1)].map(channel => channel + channel).join("").toUpperCase();
  if (/^#[\da-f]{6}$/i.test(text)) return text.toUpperCase();
  const rgb = /^rgb\(\s*([^()]*)\s*\)$/i.exec(text);
  if (!rgb) return null;
  const channels = rgb[1].includes(",") ? rgb[1].split(",").map(channel => channel.trim()) : rgb[1].trim().split(/\s+/);
  if (channels.length !== 3 || channels.some(channel => !/^\+?(?:\d+(?:\.\d*)?|\.\d+)%?$/.test(channel))) return null;
  const bytes = channels.map(channel => {
    const percent = channel.endsWith("%"), number = Number(percent ? channel.slice(0, -1) : channel);
    if (!Number.isFinite(number) || number < 0 || number > (percent ? 100 : 255)) return NaN;
    return Math.round(percent ? number * 255 / 100 : number);
  });
  if (bytes.some(Number.isNaN)) return null;
  return "#" + bytes.map(channel => channel.toString(16).padStart(2, "0")).join("").toUpperCase();
}

export function colorSource(value, titlePrefix = "色卡 ") {
  const hex = parseCaptureColor(value);
  if (!hex) return null;
  const canvas = captureCanvas(330, 220), context = canvas.getContext("2d");
  context.fillStyle = hex; context.fillRect(0, 0, 330, 160);
  context.fillStyle = "#fff"; context.fillRect(0, 160, 330, 60);
  context.font = "23px Consolas,monospace"; context.fillStyle = "#41604a"; context.fillText(hex, 20, 200);
  return { data_url: canvas.toDataURL("image/png"), width: 330, height: 220, title: titlePrefix + hex };
}
