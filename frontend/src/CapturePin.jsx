import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { CaptureIcon } from "./CaptureSurface.jsx";
import { clampCapture, normalizePinView } from "./capture-model.js";
import { renderPinnedCapture } from "./capture-renderer.js";
import "./capture-widgets.css";

export async function captureDispatch(workspace, action) {
  const result = await workspace.dispatch(action);
  if (!result?.ok) throw new Error(result?.error?.message || "操作失败，图片已保留");
  return result.data;
}

export function pinnedDimensions(record, suppliedView = record.view, pixelRatio = 1) {
  const view = normalizePinView(suppliedView);
  const swap = Math.abs(view.rotation % 180) === 90;
  const width = swap ? record.height : record.width;
  const height = swap ? record.width : record.height;
  const scale = view.thumbnail ? Math.min(160 / width, 120 / height, view.scale / pixelRatio) : view.scale / pixelRatio;
  return { width: width * scale, height: height * scale, scale };
}

// Expansion belongs to the window, not to the saved image geometry. The native
// overlay flag keeps move events from replacing the user's pin position.
export async function expandPinOverlay(workspace, record, fullScreen = false) {
  const api = globalThis.__TAURI__?.window;
  const current = api?.getCurrentWindow?.();
  if (!current) return { x: 0, y: 0 };
  await captureDispatch(workspace, { type: "capture.pin.update", pinId: record.id, view: record.view, overlay: true });
  try {
    const [size, position, factorValue, monitor] = await Promise.all([
      current.innerSize(), current.innerPosition(), current.scaleFactor(), api.currentMonitor?.(),
    ]);
    const factor = Number(factorValue) || devicePixelRatio || 1;
    const bounds = monitor || { position: { x: 0, y: 0 }, size: { width: screen.availWidth * factor, height: screen.availHeight * factor } };
    const width = fullScreen ? bounds.size.width : Math.min(bounds.size.width, Math.max(size.width, 268 * factor));
    const height = fullScreen ? bounds.size.height : Math.min(bounds.size.height, Math.max(size.height, 530 * factor));
    const x = fullScreen ? bounds.position.x : clampCapture(position.x, bounds.position.x, bounds.position.x + bounds.size.width - width);
    const y = fullScreen ? bounds.position.y : clampCapture(position.y, bounds.position.y, bounds.position.y + bounds.size.height - height);
    await current.setPosition({ type: "Physical", x: Math.round(x), y: Math.round(y) });
    await current.setSize({ type: "Physical", width: Math.round(width), height: Math.round(height) });
    return { x: (position.x - x) / factor, y: (position.y - y) / factor, width: width / factor, height: height / factor };
  } catch (error) {
    await captureDispatch(workspace, { type: "capture.pin.update", pinId: record.id, view: record.view, overlay: false }).catch(() => {});
    throw error;
  }
}

export default function CapturePin({ workspace, record, native = false, english = false, onEdit, onChange, onHidden }) {
  const t = (zh, en) => english ? en : zh;
  const [view, setView] = useState(() => normalizePinView(record.view));
  const [menu, setMenu] = useState(null);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [error, setError] = useState("");
  const [selected, setSelected] = useState(false);
  const [pixelRatio, setPixelRatio] = useState(native ? Number(globalThis.devicePixelRatio) || 1 : 1);
  const imageNode = useRef(null), menuNode = useRef(null), live = useRef(null), drag = useRef(null), wheel = useRef(0), queue = useRef(Promise.resolve()), generation = useRef(0), overlay = useRef(false);
  const pendingWrites = useRef(0), committedView = useRef(normalizePinView(record.view));
  live.current = { view, record, menu, onEdit, onChange, onHidden };
  useEffect(() => { if (!drag.current && !pendingWrites.current) { const next = normalizePinView(record.view); committedView.current = next; setView(next); } }, [record.view]);
  useEffect(() => {
    const select = event => setSelected(event.detail === record.id);
    addEventListener("xchat-capture-pin-select", select);
    return () => removeEventListener("xchat-capture-pin-select", select);
  }, [record.id]);
  useEffect(() => {
    if (!native) return undefined;
    let disposed = false;
    const refresh = () => {
      const current = globalThis.__TAURI__?.window?.getCurrentWindow?.();
      Promise.resolve(current?.scaleFactor?.() || globalThis.devicePixelRatio || 1).then(value => { if (!disposed) setPixelRatio(Number(value) || 1); }).catch(() => {});
    };
    refresh(); addEventListener("resize", refresh);
    return () => { disposed = true; removeEventListener("resize", refresh); };
  }, [native]);

  async function closeMenu(restoreFocus = false) {
    setMenu(null);
    if (native && overlay.current) {
      overlay.current = false;
      await captureDispatch(workspace, { type: "capture.pin.update", pinId: record.id, view: live.current.view, overlay: false });
      setOffset({ x: 0, y: 0 });
    }
    if (restoreFocus) imageNode.current?.focus({ preventScroll: true });
  }
  async function applyView(patch) {
    const next = normalizePinView({ ...live.current.view, ...patch });
    const version = ++generation.current;
    pendingWrites.current += 1;
    live.current.view = next;
    setView(next);
    queue.current = queue.current.catch(() => {}).then(async () => {
      try {
        const result = await captureDispatch(workspace, { type: "capture.pin.update", pinId: record.id, view: next });
        const saved = normalizePinView(result?.view || result || next);
        committedView.current = saved;
        if (version === generation.current) {
          setView(saved); live.current.view = saved;
          live.current.onChange?.({ ...record, view: saved });
        }
      } catch (cause) {
        if (version === generation.current) { setView(committedView.current); live.current.view = committedView.current; }
        setError(cause.message);
      } finally { pendingWrites.current -= 1; }
    });
    await queue.current;
  }
  async function run(action) {
    setError("");
    try {
      await closeMenu(true);
      const current = live.current.view;
      if (action === "edit") return await live.current.onEdit?.({ ...record, view: current });
      if (["copy", "original", "save"].includes(action)) {
        const canvas = await renderPinnedCapture({ ...record, view: current }, action === "original");
        const result = await captureDispatch(workspace, { type: action === "save" ? "capture.pin.save" : "capture.pin.copy", pinId: record.id, dataUrl: canvas.toDataURL("image/png") });
        if (result !== null) setError(t(action === "save" ? "已保存图像" : "已复制图像", action === "save" ? "Image saved" : "Image copied"));
        return;
      }
      if (action === "hide" || action === "destroy") {
        await captureDispatch(workspace, { type: "capture.pin.close", pinId: record.id, destroy: action === "destroy" });
        live.current.onHidden?.(record, action === "destroy");
        return;
      }
      const patches = { reset: { scale: 1, thumbnail: false }, "middle-reset": { scale: 1, opacity: 1, thumbnail: false }, rotate: { rotation: current.rotation + 90 }, "rotate-back": { rotation: current.rotation + 270 }, flip: { flipX: current.flipX * -1 }, "flip-y": { flipY: current.flipY * -1 }, thumbnail: { thumbnail: !current.thumbnail }, shadow: { shadow: !current.shadow }, through: { through: !current.through } };
      if (patches[action]) await applyView(patches[action]);
    } catch (cause) { setError(cause.message); }
  }

  async function openMenu(event) {
    event.preventDefault(); event.stopPropagation();
    const point = { x: event.clientX, y: event.clientY };
    globalThis.dispatchEvent(new CustomEvent("xchat-capture-pin-menu", { detail: record.id }));
    setSelected(true);
    imageNode.current?.focus({ preventScroll: true });
    try {
      await queue.current;
      let origin = { x: 0, y: 0, width: innerWidth, height: innerHeight };
      if (native) {
        origin = await expandPinOverlay(workspace, { ...record, view: live.current.view });
        overlay.current = true; setOffset(origin);
      }
      setMenu({ x: clampCapture(point.x + origin.x, 8, (origin.width || innerWidth) - 242), y: clampCapture(point.y + origin.y, 8, (origin.height || innerHeight) - 514) });
    } catch (cause) { setError(cause.message); }
  }

  useEffect(() => {
    const element = imageNode.current;
    if (!element) return undefined;
    if (native) element.focus({ preventScroll: true });
    const scroll = event => {
      event.preventDefault(); event.stopPropagation();
      if (live.current.menu) return;
      wheel.current += event.deltaY * (event.deltaMode ? 40 : 1);
      if (Math.abs(wheel.current) < 35) return;
      const direction = wheel.current > 0 ? -1 : 1;
      wheel.current = 0;
      const current = live.current.view;
      void applyView(event.ctrlKey || event.metaKey ? { opacity: clampCapture(current.opacity + direction * .05, .15, 1) } : { scale: clampCapture(Math.round(current.scale * (direction > 0 ? 1.1 : 1 / 1.1) * 1000) / 1000, .1, 8), thumbnail: false });
    };
    element.addEventListener("wheel", scroll, { passive: false });
    return () => element.removeEventListener("wheel", scroll);
  }, [record.id]);

  useEffect(() => {
    if (!menu) return undefined;
    const dismiss = event => {
      if (!menuNode.current?.contains(event.target)) void closeMenu().catch(cause => setError(cause.message));
    };
    const other = event => { if (event.detail !== record.id) void closeMenu().catch(cause => setError(cause.message)); };
    const blur = () => void closeMenu().catch(cause => setError(cause.message));
    addEventListener("pointerdown", dismiss); addEventListener("blur", blur); addEventListener("xchat-capture-pin-menu", other);
    menuNode.current?.querySelector("button")?.focus();
    return () => { removeEventListener("pointerdown", dismiss); removeEventListener("blur", blur); removeEventListener("xchat-capture-pin-menu", other); };
  }, [menu, record.id]);
  useEffect(() => { if (!error) return undefined; const timer = setTimeout(() => setError(""), 4500); return () => clearTimeout(timer); }, [error]);

  function keyDown(event) {
    if (event.isComposing || event.target.matches("input,textarea,select")) return;
    if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); void (live.current.menu ? closeMenu(true).catch(cause => setError(cause.message)) : run(event.shiftKey ? "destroy" : "hide")); return; }
    if (live.current.menu && event.key === " " && event.target.closest("button")) return;
    if (live.current.menu && ["ArrowDown", "ArrowUp"].includes(event.key)) {
      const buttons = [...menuNode.current.querySelectorAll("button:not(:disabled)")];
      const index = buttons.indexOf(document.activeElement), direction = event.key === "ArrowDown" ? 1 : -1;
      event.preventDefault(); buttons[(index + direction + buttons.length) % buttons.length]?.focus(); return;
    }
    const shortcut = event.ctrlKey || event.metaKey;
    if (!live.current.menu && !shortcut && ["+", "=", "-", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(event.key)) {
      event.preventDefault(); event.stopPropagation();
      const current = live.current.view;
      if (["+", "=", "-"].includes(event.key)) void applyView({ scale: clampCapture(Math.round(current.scale * (event.key === "-" ? 1 / 1.1 : 1.1) * 1000) / 1000, .1, 8), thumbnail: false });
      else {
        const delta = { ArrowUp: [0, -1], ArrowDown: [0, 1], ArrowLeft: [-1, 0], ArrowRight: [1, 0] }[event.key];
        const size = pinnedDimensions(record, current, pixelRatio);
        void applyView({ x: native ? current.x + delta[0] : clampCapture(current.x + delta[0], -size.width + 28, innerWidth - 28), y: native ? current.y + delta[1] : clampCapture(current.y + delta[1], 0, innerHeight - 28) });
      }
      return;
    }
    const action = shortcut && event.key.toLowerCase() === "c" ? "copy" : shortcut && event.key.toLowerCase() === "s" ? "save" : event.key === " " ? "edit" : ({ 1: "rotate", 2: "rotate-back", 3: "flip", 4: "flip-y" })[event.key];
    if (action) { event.preventDefault(); event.stopPropagation(); void run(action); }
  }
  const dims = pinnedDimensions(record, view, pixelRatio);
  const left = native ? offset.x : view.x, top = native ? offset.y : view.y;
  const menuButton = (action, zh, en, key) => <button type="button" role="menuitem" data-pin-action={action} onClick={() => void run(action)}><span>{t(zh, en)}</span>{key && <kbd>{key}</kbd>}</button>;

  return <>
    <div ref={imageNode} data-capture-pin={record.id} className={`cap-pin cap-live-pin${native ? " cap-native-pin" : ""}${selected ? " selected" : ""}${!view.shadow ? " no-shadow" : ""}${view.through && !native ? " through" : ""}`} tabIndex={0} aria-label={t("贴图 ", "Pinned image ") + (record.title || record.file_name || "XChat")} style={{ left, top, width: dims.width, height: dims.height, zIndex: selected ? 40 : 3 }} onFocus={() => { setSelected(true); dispatchEvent(new CustomEvent("xchat-capture-pin-select", { detail: record.id })); }} onBlur={() => { if (!live.current.menu) setSelected(false); }} onKeyDown={keyDown} onContextMenu={openMenu} onDoubleClick={event => { event.preventDefault(); void run(event.shiftKey ? "thumbnail" : "hide"); }}
      onPointerDown={event => {
        if (event.button === 1) { event.preventDefault(); event.stopPropagation(); imageNode.current.focus({ preventScroll: true }); void run("middle-reset"); return; }
        if (event.button !== 0) return;
        setSelected(true); imageNode.current.focus({ preventScroll: true });
        if (native) { if (event.detail < 2) void globalThis.__TAURI__?.window?.getCurrentWindow?.().startDragging?.().catch(cause => setError(cause.message)); return; }
        drag.current = { clientX: event.clientX, clientY: event.clientY, x: live.current.view.x, y: live.current.view.y, moved: false };
        event.currentTarget.setPointerCapture(event.pointerId);
      }} onPointerMove={event => {
        const start = drag.current; if (!start) return;
        if (!start.moved && Math.hypot(event.clientX - start.clientX, event.clientY - start.clientY) < 4) return;
        start.moved = true;
        const next = { ...live.current.view, x: clampCapture(start.x + event.clientX - start.clientX, -dims.width + 28, innerWidth - 28), y: clampCapture(start.y + event.clientY - start.clientY, 0, innerHeight - 28) };
        live.current.view = next; setView(next);
      }} onPointerUp={() => { const start = drag.current; drag.current = null; if (start?.moved) void applyView({ x: live.current.view.x, y: live.current.view.y }); }} onPointerCancel={() => { const start = drag.current; drag.current = null; if (start) { const restored = { ...live.current.view, x: start.x, y: start.y }; live.current.view = restored; setView(restored); } }} onAuxClick={event => { if (event.button === 1) event.preventDefault(); }}>
      <img className="cap-pin-image" src={record.data_url} alt={record.title || t("截图", "Capture")} draggable="false" style={{ position: "absolute", width: record.width * dims.scale, height: record.height * dims.scale, left: (dims.width - record.width * dims.scale) / 2, top: (dims.height - record.height * dims.scale) / 2, opacity: view.opacity, transform: `rotate(${view.rotation}deg) scale(${view.flipX},${view.flipY})` }} />
      <span className="cap-pin-label">{Math.round(view.scale * 100)}% · {Math.round(view.opacity * 100)}% · {t(view.thumbnail ? "缩略图" : "右键操作", view.thumbnail ? "Thumbnail" : "Right-click")}</span>
    </div>
    {menu && createPortal(<div className="cap-root cap-pin-menu-layer"><div ref={menuNode} className="cap-pin-menu cap-live-pin-menu" role="menu" aria-label={t("贴图操作", "Pinned image actions")} style={{ left: menu.x, top: menu.y }} onKeyDown={keyDown} onContextMenu={event => event.preventDefault()}>
      {menuButton("edit", "标注图片", "Annotate image", "Space")}{menuButton("copy", "复制当前图像", "Copy current image", "Ctrl+C")}{menuButton("original", "复制原始图像", "Copy original image")}{menuButton("save", "保存图像", "Save image", "Ctrl+S")}<hr />
      {menuButton("reset", "原始大小", "Original size", "100%")}{menuButton("rotate", "顺时针旋转", "Rotate clockwise", "1")}{menuButton("flip", "水平翻转", "Flip horizontally", "3")}{menuButton("thumbnail", view.thumbnail ? "恢复完整图片" : "切换缩略图", view.thumbnail ? "Restore full image" : "Thumbnail")}
      <label>{t("透明度", "Opacity")}<input type="range" min="15" max="100" value={Math.round(view.opacity * 100)} aria-label={t("贴图透明度", "Pinned image opacity")} onChange={event => { const opacity = Number(event.target.value) / 100; setView(previous => ({ ...previous, opacity })); live.current.view = { ...live.current.view, opacity }; }} onPointerUp={() => { const opacity = live.current.view.opacity; void closeMenu().then(() => applyView({ opacity })).catch(cause => setError(cause.message)); }} onKeyUp={event => { if (event.key.startsWith("Arrow")) void closeMenu().then(() => applyView({ opacity: live.current.view.opacity })).catch(cause => setError(cause.message)); }} /><output>{Math.round(view.opacity * 100)}%</output></label>
      <button type="button" role="menuitemcheckbox" aria-checked={view.shadow} data-pin-action="shadow" onClick={() => void run("shadow")}><span>{t("窗口阴影", "Window shadow")}</span><span className="cap-menu-check"><CaptureIcon name="check" /></span></button><hr />
      {native && menuButton("through", view.through ? "关闭鼠标穿透" : "启用鼠标穿透", view.through ? "Disable click-through" : "Enable click-through")}
      <label>{t("所属分组", "Group")}<select value={view.group} aria-label={t("贴图分组", "Pin group")} onChange={event => { const group = event.target.value; void closeMenu().then(() => applyView({ group })).catch(cause => setError(cause.message)); }}><option value="默认">{t("默认", "Default")}</option><option value="设计参考">{t("设计参考", "References")}</option></select></label><hr />
      {menuButton("hide", "隐藏", "Hide", "Esc")}<button type="button" role="menuitem" className="danger" data-pin-action="destroy" onClick={() => void run("destroy")}><span>{t("销毁", "Destroy")}</span><kbd>Shift+Esc</kbd></button>
    </div></div>, document.body)}
    {error && <div className="cap-pin-feedback" role="status">{error}</div>}
  </>;
}
