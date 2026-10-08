import { useEffect, useRef, useState } from "react";
import { addCaptureOperation, createCaptureHistory, moveCaptureSelection, normalizeCaptureSelection, placeCaptureToolbar, redoCaptureOperation, removeCaptureOperation, replaceCaptureOperation, resizeCaptureSelection, undoCaptureOperation } from "./capture-drawing.js";
import { CAPTURE_COLORS, CAPTURE_FONTS, CAPTURE_TOOLS, DEFAULT_TOOL_SIZES, captureDisplayRect, captureId, capturePoint, captureSizeOptions, captureTextEditorPlacement, captureView, clampCapture, constrainCapturePoint, insideCapture, moveCaptureAnchor, stepCaptureSize } from "./capture-model.js";
import { captureCanvas, captureImage, captureTextAt, captureTextLayout, exportCapture, paintCaptureOperation, renderCaptureAnnotations } from "./capture-renderer.js";
import { captureSettings, saveCaptureRecord, saveCaptureSettings } from "./capture-library.js";
import { CAPTURE_ICONS } from "./capture-icons.js";
import "./capture-surface.css";

export function CaptureIcon({ name }) {
  return <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false" dangerouslySetInnerHTML={{ __html: CAPTURE_ICONS[name] || CAPTURE_ICONS.capture }} />;
}
const handles = [["nw", 0, 0], ["n", .5, 0], ["ne", 1, 0], ["e", 1, .5], ["se", 1, 1], ["s", .5, 1], ["sw", 0, 1], ["w", 0, .5]];
const clone = value => JSON.parse(JSON.stringify(value));
export default function CaptureSurface({ workspace, pending: supplied, onClose, onPinUpdated, english = false }) {
  const t = (zh, en) => english ? en : zh;
  const [state, setState] = useState(() => ({ pending: null, base: null, region: null, history: createCaptureHistory(), draft: null, editing: null, tool: "select", sizes: { ...DEFAULT_TOOL_SIZES }, color: CAPTURE_COLORS[0], fontFamily: CAPTURE_FONTS[0][0], busy: false, error: "", cursor: null, hover: null, toolsVisible: true, preview: false, alt: false, rgb: false, viewport: { width: innerWidth, height: innerHeight }, settings: captureSettings() }));
  const live = useRef(state), stage = useRef(null), canvas = useRef(null), textElement = useRef(null), toolbar = useRef(null), magnifier = useRef(null), gesture = useRef(null), wheel = useRef(0), previewTimer = useRef(null), composing = useRef(false), insertion = useRef(null), sourceRequest = useRef(null);
  const [toolbarSize, setToolbarSize] = useState({ width: 644, height: 82 });
  const update = patch => { live.current = { ...live.current, ...patch }; setState(live.current); };
  const history = next => update({ history: typeof next === "function" ? next(live.current.history) : next });
  const view = state.base ? captureView(state.base, state.viewport) : { scale: 1, x: 0, y: 0 };
  const sourcePoint = event => capturePoint({ x: event.clientX, y: event.clientY }, captureView(live.current.base, live.current.viewport));
  const add = operation => history(current => addCaptureOperation(current, operation));

  function commitText() {
    if (composing.current) return;
    const edit = live.current.editing;
    if (!edit) return;
    const { originalId, ...operation } = edit;
    if (operation.text.trim()) {
      const original = live.current.history.operations.find(item => item.id === originalId);
      if (originalId && original) {
        if (JSON.stringify(operation) !== JSON.stringify(original)) history(current => replaceCaptureOperation(current, originalId, operation));
      } else add(operation);
    } else if (originalId) history(current => removeCaptureOperation(current, originalId));
    update({ editing: null });
  }
  function deleteText() {
    const edit = live.current.editing;
    if (!edit) return;
    commitText();
    history(current => removeCaptureOperation(current, edit.id));
  }
  function finishPolyline() {
    const draft = live.current.draft;
    if (draft?.tool !== "polyline") return;
    if (draft.points.length > 1) { const { end, ...operation } = draft; add(operation); }
    update({ draft: null });
  }
  function changeTool(tool) { if (composing.current) return; commitText(); finishPolyline(); update({ tool }); }
  function restoreInsertion() {
    requestAnimationFrame(() => {
      if (!live.current.editing || !textElement.current) return;
      textElement.current.focus({ preventScroll: true });
      if (insertion.current) textElement.current.setSelectionRange(...insertion.current);
    });
  }
  function setParameter(value, color) {
    const s = live.current;
    const patch = color ? { color: value } : { sizes: { ...s.sizes, [s.tool]: Number(value) } };
    if (s.editing) patch.editing = { ...s.editing, ...(color ? { color: value } : { fontSize: Number(value), size: Number(value) }) };
    if (s.draft) patch.draft = { ...s.draft, ...(color ? { color: value } : { size: Number(value) }) };
    update(patch);
    if (s.editing) restoreInsertion();
  }
  async function close() {
    if (live.current.busy) return;
    if (!live.current.pending?.pin_id) await workspace.dispatch({ type: "capture.cancel", sourceSessionId: live.current.pending?.session_id });
    if (onClose) onClose();
    else globalThis.__TAURI__?.window?.getCurrentWindow?.().close?.();
  }
  async function output(action) {
    if (live.current.busy || !live.current.region || composing.current) return;
    commitText(); finishPolyline();
    const s = live.current;
    const previousRegion = s.settings.lastRegion;
    update({ busy: true, error: "" });
    try {
      const rendered = exportCapture(s.base, s.history.operations, s.region);
      const dataUrl = rendered.toDataURL("image/png");
      const document = { version: 1, base: s.base.toDataURL("image/png"), region: clone(s.region), operations: clone(s.history.operations) };
      const record = { id: captureId(), kind: "history", data_url: dataUrl, document, width: rendered.width, height: rendered.height, title: `XChat ${new Date().toLocaleString()}` };
      // Persist the editable snapshot before native output closes its window.
      await saveCaptureRecord(record);
      const pinId = s.pending.pin_id || captureId();
      saveCaptureSettings({ lastRegion: { ...s.region, imageWidth: s.base.width, imageHeight: s.base.height } });
      const result = await workspace.dispatch({ type: `capture.${action}`, dataUrl, document, pinId: action === "pin" ? pinId : s.pending.pin_id, conversationId: s.pending.conversation_id, sourceSessionId: s.pending.session_id, width: rendered.width, height: rendered.height });
      if (!result.ok) throw new Error(result.error.message);
      if (action === "save" && result.data === null) { saveCaptureSettings({ lastRegion: previousRegion }); update({ busy: false }); return; }
      if (action === "finish" && result.data) {
        const event = { type: "capture-ready", attachment: { ...result.data, preview_url: dataUrl } };
        globalThis.dispatchEvent(new CustomEvent("xchat-capture-ready", { detail: event }));
        if (globalThis.BroadcastChannel) { const channel = new BroadcastChannel("xchat-capture"); channel.postMessage(event); channel.close(); }
      }
      update({ busy: false });
      if (s.pending.pin_id) { onPinUpdated?.(); onClose?.(); }
      else if (onClose) onClose();
      else globalThis.__TAURI__?.window?.getCurrentWindow?.().close?.();
    } catch (error) {
      try { saveCaptureSettings({ lastRegion: previousRegion }); } catch { /* Preserve the original output error. */ }
      update({ busy: false, error: error.message || t("输出失败，编辑内容已保留", "Output failed; edits are preserved") });
    }
  }

  useEffect(() => {
    let disposed = false;
    (async () => {
      try {
        let request = sourceRequest.current;
        // Share the request across StrictMode's effect replay, only for this mount and source.
        if (!request || request.workspace !== workspace || request.supplied !== supplied) {
          request = { workspace, supplied, promise: Promise.resolve(supplied ? { ok: true, data: supplied } : workspace.dispatch({ type: "capture.pending" })) };
          sourceRequest.current = request;
        }
        const result = await request.promise;
        if (disposed) return;
        if (!result.ok || !result.data?.data_url) throw new Error(result.error?.message || t("待编辑截图不可用", "Capture unavailable"));
        const pending = result.data, document = pending.document;
        const image = await captureImage(document?.base || pending.data_url);
        if (disposed) return;
        const base = captureCanvas(image.naturalWidth, image.naturalHeight);
        base.getContext("2d").drawImage(image, 0, 0);
        const settings = captureSettings();
        let region = document?.region || (pending.pin_id ? { x: 0, y: 0, width: base.width, height: base.height } : null);
        if (!region && settings.presetW > 0 && settings.presetH > 0) region = { x: 0, y: 0, width: Math.min(base.width, settings.presetW), height: Math.min(base.height, settings.presetH) };
        update({ pending, base, region, history: { operations: document?.operations || [], undo: [], redo: [] }, settings });
      } catch (error) { if (!disposed) update({ error: error.message }); }
    })();
    const resize = () => update({ viewport: { width: innerWidth, height: innerHeight } });
    addEventListener("resize", resize);
    return () => { disposed = true; removeEventListener("resize", resize); clearTimeout(previewTimer.current); };
  }, [workspace, supplied]);

  const annotations = useRef(null);
  useEffect(() => {
    if (!state.base) return;
    annotations.current = renderCaptureAnnotations(state.base, state.history.operations, state.editing?.originalId);
  }, [state.base, state.history, state.editing?.originalId]);
  useEffect(() => {
    if (!state.base || !canvas.current || !annotations.current) return;
    const target = canvas.current, context = target.getContext("2d");
    target.width = state.base.width; target.height = state.base.height;
    context.drawImage(state.base, 0, 0);
    if (state.draft) {
      const layer = captureCanvas(target.width, target.height), c = layer.getContext("2d");
      c.drawImage(annotations.current, 0, 0);
      const draft = state.draft.tool === "polyline" && state.draft.end ? { ...state.draft, points: [...state.draft.points, state.draft.end] } : state.draft;
      paintCaptureOperation(c, draft, state.base);
      context.drawImage(layer, 0, 0);
    } else context.drawImage(annotations.current, 0, 0);
    if (state.editing) {
      const layout = captureTextLayout(state.editing, context);
      const placement = captureTextEditorPlacement(state.editing.start, layout, captureView(state.base, state.viewport), state.viewport);
      if (placement.relocated) paintCaptureOperation(context, state.editing, state.base);
    }
    const r = state.region || state.hover;
    context.fillStyle = "rgba(20, 37, 28, .46)";
    if (!r) context.fillRect(0, 0, target.width, target.height);
    else {
      context.fillRect(0, 0, target.width, r.y);
      context.fillRect(0, r.y + r.height, target.width, target.height - r.y - r.height);
      context.fillRect(0, r.y, r.x, r.height);
      context.fillRect(r.x + r.width, r.y, target.width - r.x - r.width, r.height);
    }
  }, [state.base, state.region, state.hover, state.history, state.draft, state.editing, state.viewport]);
  useEffect(() => {
    const element = toolbar.current;
    if (!element) return;
    const observer = new ResizeObserver(() => setToolbarSize({ width: element.offsetWidth, height: element.offsetHeight }));
    observer.observe(element); return () => observer.disconnect();
  }, [Boolean(state.region)]);
  useEffect(() => {
    if (state.editing) textElement.current?.focus({ preventScroll: true });
  }, [state.editing?.id]);
  useEffect(() => {
    if (!state.cursor || !state.base || !magnifier.current) return;
    const p = state.cursor.source, context = magnifier.current.getContext("2d");
    context.imageSmoothingEnabled = false;
    context.clearRect(0, 0, 128, 96);
    context.drawImage(state.base, p.x - 16, p.y - 12, 32, 24, 0, 0, 128, 96);
    context.strokeStyle = "#f04444"; context.lineWidth = 1;
    context.beginPath(); context.moveTo(64, 0); context.lineTo(64, 96); context.moveTo(0, 48); context.lineTo(128, 48); context.stroke();
  }, [state.cursor, state.base]);

  function pointerDown(event) {
    const s = live.current;
    if (!s.base || s.busy || event.target.closest(".cap-tools,.cap-error,input,select,textarea")) return;
    if (event.button === 1) { event.preventDefault(); void output("pin"); return; }
    if (event.button !== 0) return;
    const point = sourcePoint(event), client = { x: event.clientX, y: event.clientY };
    const edge = event.target.closest("[data-text-edge]");
    const handle = event.target.dataset.handle;
    if (edge && s.editing) {
      const edit = clone(s.editing);
      commitText();
      update({ editing: { ...edit, originalId: edit.id } });
      gesture.current = { kind: "text", origin: { ...edit, originalId: edit.id }, point, client, moved: false };
    }
    else if (s.editing) { commitText(); return; }
    else if (handle) gesture.current = { kind: "resize", handle, region: s.region };
    else if (!s.region) {
      gesture.current = { kind: "create", point, client, hover: s.hover };
      update({ region: { x: point.x, y: point.y, width: 0, height: 0 }, hover: null });
    } else if (insideCapture(point, s.region)) {
      const hit = ["text", "select"].includes(s.tool) && captureTextAt(s.history.operations, point, canvas.current.getContext("2d"));
      if (hit || s.tool === "text") {
        const edit = hit ? { ...clone(hit), originalId: hit.id } : { id: captureId(), tool: "text", start: point, text: "", color: s.color, size: s.sizes.text, fontSize: s.sizes.text, fontFamily: s.fontFamily };
        update({ editing: edit, tool: "text", color: edit.color, fontFamily: edit.fontFamily, sizes: { ...s.sizes, text: edit.fontSize } });
        insertion.current = null;
        event.preventDefault(); return;
      }
      if (s.tool === "select") gesture.current = { kind: "move", point, region: clone(s.region) };
      else if (s.tool === "polyline") {
        const draft = s.draft || { id: captureId(), tool: s.tool, size: s.sizes[s.tool], color: s.color, points: [] };
        update({ draft: { ...draft, points: [...draft.points, constrainCapturePoint(draft.points.at(-1) || point, point, s.tool, event.shiftKey)], end: null } });
      } else {
        gesture.current = { kind: "draw", point };
        const op = { id: captureId(), tool: s.tool, size: s.sizes[s.tool], color: s.color };
        update({ draft: ["pen", "marker", "eraser"].includes(s.tool) ? { ...op, points: [point] } : { ...op, start: point, end: point } });
      }
    } else return;
    event.preventDefault();
    stage.current?.setPointerCapture(event.pointerId);
  }
  function pointerMove(event) {
    const s = live.current;
    if (!s.base) return;
    const point = sourcePoint(event), g = gesture.current;
    if (event.target.closest(".cap-tools") && !g) return;
    const px = clampCapture(Math.floor(point.x), 0, s.base.width - 1), py = clampCapture(Math.floor(point.y), 0, s.base.height - 1);
    const color = s.base.getContext("2d").getImageData(px, py, 1, 1).data;
    const cursor = { x: event.clientX, y: event.clientY, source: { x: px, y: py }, color: "#" + [...color].slice(0, 3).map(n => n.toString(16).padStart(2, "0")).join(""), rgb: `rgb(${[...color].slice(0, 3).join(", ")})` };
    const patch = { cursor };
    if (g?.kind === "create") patch.region = normalizeCaptureSelection(g.point, point, s.base);
    if (g?.kind === "move") patch.region = moveCaptureSelection(g.region, { x: point.x - g.point.x, y: point.y - g.point.y }, s.base);
    if (g?.kind === "resize") patch.region = resizeCaptureSelection(g.region, g.handle, point, s.base, 1);
    if (g?.kind === "draw" && s.draft) patch.draft = s.draft.points ? { ...s.draft, points: [...s.draft.points, point] } : { ...s.draft, end: constrainCapturePoint(g.point, point, s.tool, event.shiftKey) };
    if (g?.kind === "text") {
      if (Math.hypot(event.clientX - g.client.x, event.clientY - g.client.y) >= 4) g.moved = true;
      if (g.moved) patch.editing = { ...g.origin, start: moveCaptureAnchor(g.origin.start, g.point, point, s.base, captureTextLayout(g.origin, canvas.current.getContext("2d"))) };
    }
    if (!g && s.draft?.tool === "polyline") patch.draft = { ...s.draft, end: constrainCapturePoint(s.draft.points.at(-1), point, "polyline", event.shiftKey) };
    if (!g && !s.region && s.settings.detect) patch.hover = (s.pending.regions || []).find(r => insideCapture(point, r)) || { x: 0, y: 0, width: s.base.width, height: s.base.height };
    update(patch);
  }
  function pointerUp(event) {
    const g = gesture.current, s = live.current;
    gesture.current = null;
    if (!g) return;
    const cancelled = event.type === "pointercancel";
    if (g.kind === "text") {
      if (cancelled) update({ editing: g.origin });
      else if (g.moved) {
        const edit = s.editing; commitText(); update({ editing: { ...edit, originalId: edit.id } });
      }
    } else if (g.kind === "draw") { if (!cancelled && s.draft) add(s.draft); update({ draft: null }); }
    else if (g.kind === "create") {
      if (cancelled) update({ region: null });
      else if (Math.hypot(event.clientX - g.client.x, event.clientY - g.client.y) < 4) update({ region: g.hover || { x: 0, y: 0, width: s.base.width, height: s.base.height } });
      else if (s.region.width < 1 || s.region.height < 1) update({ region: null });
    } else if (cancelled) update({ region: g.region });
    if (stage.current?.hasPointerCapture(event.pointerId)) stage.current.releasePointerCapture(event.pointerId);
  }
  function contextMenu(event) {
    event.preventDefault();
    if (live.current.draft?.tool === "polyline") finishPolyline();
    else if (live.current.editing) commitText();
    else if (live.current.region) update({ region: null, tool: "select" });
    else void close();
  }
  useEffect(() => {
    function onWheel(event) {
      const s = live.current;
      if (!s.region || s.tool === "select" || s.busy || event.target.closest("select")) return;
      event.preventDefault();
      wheel.current += event.deltaMode === 1 ? event.deltaY * 16 : event.deltaY;
      if (Math.abs(wheel.current) < 12) return;
      const size = stepCaptureSize(s.tool, s.sizes[s.tool], wheel.current < 0 ? 1 : -1);
      wheel.current = 0; setParameter(size);
      update({ preview: true }); clearTimeout(previewTimer.current);
      previewTimer.current = setTimeout(() => update({ preview: false }), 850);
    }
    const target = stage.current;
    target?.addEventListener("wheel", onWheel, { passive: false });
    return () => target?.removeEventListener("wheel", onWheel);
  }, []);
  useEffect(() => {
    function keydown(event) {
      const s = live.current;
      if (event.isComposing || event.keyCode === 229 || composing.current || s.busy) return;
      const key = event.key.toLowerCase(), mod = event.ctrlKey || event.metaKey;
      if (event.target === textElement.current) {
        if (event.key === "Delete") { event.preventDefault(); deleteText(); }
        else if (event.key === "Escape") { event.preventDefault(); update({ editing: null }); }
        return;
      }
      if (event.target.closest?.("input,textarea,select")) return;
      let handled = true;
      if (event.key === "Escape") {
        if (s.editing) update({ editing: null });
        else if (s.draft) update({ draft: null });
        else void close();
      } else if (event.key === "Alt") update({ alt: true });
      else if (event.key === "Shift" && !event.repeat && s.cursor && !s.editing) update({ rgb: !s.rgb });
      else if (mod && key === "a" && s.base) update({ region: { x: 0, y: 0, width: s.base.width, height: s.base.height }, hover: null });
      else if (mod && key === "z") { commitText(); history(event.shiftKey ? createCaptureHistory() : undoCaptureOperation(live.current.history)); }
      else if (mod && key === "y") { commitText(); history(redoCaptureOperation(live.current.history)); }
      else if (mod && key === "c") void output("copy");
      else if (mod && key === "s") void output("save");
      else if (mod && key === "t") void output("pin");
      else if (event.key === "Enter") { if (s.draft?.tool === "polyline") finishPolyline(); else if (s.editing) commitText(); else void output("copy"); }
      else if (event.key === " ") update({ toolsVisible: !s.toolsVisible });
      else if (key === "r" && s.settings.lastRegion?.imageWidth === s.base?.width && s.settings.lastRegion?.imageHeight === s.base?.height) update({ region: s.settings.lastRegion });
      else if (key === "c" && s.cursor) void workspace.dispatch({ type: "capture.color.copy", text: s.rgb ? s.cursor.rgb : s.cursor.color.toUpperCase() }).then(result => { if (!result.ok) update({ error: result.error.message }); });
      else if (key === "t" && !mod) changeTool("text");
      else if (key === "b" && !mod) changeTool("pen");
      else if (event.key.startsWith("Arrow") && s.region) {
        const dx = event.key === "ArrowLeft" ? -1 : event.key === "ArrowRight" ? 1 : 0, dy = event.key === "ArrowUp" ? -1 : event.key === "ArrowDown" ? 1 : 0;
        if (mod || event.shiftKey) {
          const amount = event.shiftKey ? -1 : 1, r = { ...s.region };
          if (dx < 0) { r.x -= amount; r.width += amount; }
          if (dx > 0) r.width += amount;
          if (dy < 0) { r.y -= amount; r.height += amount; }
          if (dy > 0) r.height += amount;
          update({ region: normalizeCaptureSelection({ x: r.x, y: r.y }, { x: r.x + Math.max(1, r.width), y: r.y + Math.max(1, r.height) }, s.base) });
        } else update({ region: moveCaptureSelection(s.region, { x: dx, y: dy }, s.base) });
      } else handled = false;
      if (handled) { event.preventDefault(); event.stopImmediatePropagation(); }
    }
    const keyup = event => { if (event.key === "Alt") update({ alt: false }); };
    const blur = () => update({ alt: false });
    addEventListener("keydown", keydown, true); addEventListener("keyup", keyup, true); addEventListener("blur", blur);
    return () => { removeEventListener("keydown", keydown, true); removeEventListener("keyup", keyup, true); removeEventListener("blur", blur); };
  });

  const display = state.region && captureDisplayRect(state.region, view);
  const placement = display && placeCaptureToolbar(display, toolbarSize, state.viewport, 10, 8);
  const range = captureSizeOptions(state.tool), size = state.sizes[state.tool];
  const editLayout = state.editing && canvas.current ? captureTextLayout(state.editing, canvas.current.getContext("2d")) : null;
  const editRect = editLayout && captureTextEditorPlacement(state.editing.start, editLayout, view, state.viewport);
  if (editRect && placement && placement.left < editRect.left + editRect.width + 10 && placement.left + toolbarSize.width > editRect.left - 10 && placement.top < editRect.top + editRect.height + 10 && placement.top + toolbarSize.height > editRect.top - 10) {
    const above = editRect.top - toolbarSize.height - 12;
    placement.top = above >= 8 ? above : Math.min(editRect.top + editRect.height + 12, state.viewport.height - toolbarSize.height - 8);
  }
  const actionButton = (action, icon, label, disabled = false, className = "") => <button type="button" className={`cap-tool ${className}`} data-capture-action={action} title={label} aria-label={label} disabled={disabled || state.busy} onClick={() => action === "undo" || action === "redo" ? (commitText(), history(action === "undo" ? undoCaptureOperation(live.current.history) : redoCaptureOperation(live.current.history))) : action === "cancel" ? void close() : void output(action)}><CaptureIcon name={icon} /></button>;
  return <section className="cap-root cap-is-capturing capture-production" aria-label={t("截图编辑器", "Capture editor")}>
    <div className="cap-desktop" ref={stage} onPointerDown={pointerDown} onPointerMove={pointerMove} onPointerUp={pointerUp} onPointerCancel={pointerUp} onContextMenu={contextMenu} onDoubleClick={event => { if (event.target === canvas.current && !live.current.editing && live.current.tool === "select") void output("copy"); }}>
      <div className="cap-capture">
        <canvas ref={canvas} aria-label={t("截图画布", "Capture canvas")} style={state.base ? { left: view.x, top: view.y, width: state.base.width * view.scale, height: state.base.height * view.scale } : undefined} />
        {display && <div className={`cap-selection${display.y < 30 ? " near-top" : ""}`} style={{ left: display.x, top: display.y, width: display.width, height: display.height }}>
          <span className="cap-dimensions">{Math.round(state.region.width)} × {Math.round(state.region.height)} px</span>
          {handles.map(([handle, x, y]) => <i key={handle} className="cap-handle" data-handle={handle} style={{ left: `${x * 100}%`, top: `${y * 100}%`, cursor: `${handle}-resize` }} />)}
        </div>}
        {!display && <div className="cap-capture-hint">{state.base ? t("拖动框选 · 单击选择识别区域 · Ctrl+A 全选 · Esc 取消", "Drag to select · Click a detected region · Ctrl+A select all · Esc cancel") : t("正在读取截图…", "Loading capture…")}</div>}
        {display && state.toolsVisible && <div ref={toolbar} className="cap-tools" style={{ left: placement.left, top: placement.top }} role="toolbar" aria-label={t("截图工具", "Capture tools")}>
          <div className="cap-tool-row">{CAPTURE_TOOLS.map(([tool, zh, en]) => <button type="button" key={tool} className={`cap-tool${state.tool === tool ? " active" : ""}`} data-capture-tool={tool} title={t(zh, en)} aria-label={t(zh, en)} aria-pressed={state.tool === tool} disabled={state.busy} onClick={() => changeTool(tool)}><CaptureIcon name={tool} /></button>)}
            <span className="cap-separator" />{actionButton("undo", "undo", t("撤销 Ctrl+Z", "Undo Ctrl+Z"), !state.history.undo.length)}{actionButton("redo", "redo", t("重做 Ctrl+Y", "Redo Ctrl+Y"), !state.history.redo.length)}<span className="cap-separator" />
            {actionButton("pin", "pin", t("贴图 Ctrl+T", "Pin Ctrl+T"))}{actionButton("save", "save", t("保存 Ctrl+S", "Save Ctrl+S"))}{actionButton("finish", "send", t("加入聊天草稿，不自动发送", "Add to chat draft"), !state.pending?.conversation_id)}{actionButton("cancel", "close", t("取消 Esc", "Cancel Esc"))}{actionButton("copy", "check", t("完成 · 复制截图", "Done · Copy capture"), false, "cap-finish")}
          </div>
          <div className="cap-style-row"><span className="cap-tool-name">{CAPTURE_TOOLS.find(item => item[0] === state.tool)?.[english ? 2 : 1]}</span>
            {state.tool === "select" ? <span id="capSelectionHint">{t("拖动调整 · 方向键微调", "Drag to adjust · Arrow keys to nudge")}</span> : <div className="cap-parameters">
              <div className="cap-swatches">{CAPTURE_COLORS.map(color => <button key={color} type="button" className={`cap-swatch${state.color === color ? " active" : ""}`} style={{ background: color }} aria-label={`${t("颜色", "Color")} ${color}`} onPointerDown={event => event.preventDefault()} onClick={() => setParameter(color, true)} />)}</div>
              <input type="color" value={state.color} aria-label={t("自定义颜色", "Custom color")} onChange={event => setParameter(event.target.value, true)} /><span className="cap-property-separator" />
              <span>{t(range.zh, range.en)}</span>{state.tool !== "text" && <span className="cap-stroke-sample"><i style={{ height: Math.min(size, 16), background: state.color }} /></span>}
              <input type="range" aria-label={t(range.zh, range.en)} min={range.min} max={range.max} step={range.step} value={size} onChange={event => setParameter(event.target.value)} /><output>{size} px</output>
              {state.tool === "text" && <select aria-label={t("字体", "Font")} value={state.fontFamily} onChange={event => { update({ fontFamily: event.target.value, editing: live.current.editing ? { ...live.current.editing, fontFamily: event.target.value } : null }); restoreInsertion(); }}>{CAPTURE_FONTS.map(([value, zh, en]) => <option key={value} value={value}>{t(zh, en)}</option>)}</select>}
              <span className="cap-wheel-hint">{t("滚轮调节", "Scroll to adjust")}</span></div>}
            <button type="button" className="cap-selection-reset" onClick={() => { commitText(); finishPolyline(); update({ region: null, tool: "select" }); }}>{t("重新框选", "Select again")}</button>
          </div></div>}
        {editLayout && <><textarea ref={textElement} className={`cap-text-editor${editRect.relocated ? " relocated" : ""}`} data-anchor-x={state.editing.start.x} data-anchor-y={state.editing.start.y} aria-label={t("编辑标注文字，回车换行，点击外部完成，Delete 删除标注", "Edit annotation; Enter for new line; click outside to finish; Delete removes annotation")} spellCheck={false} wrap="off" value={state.editing.text} style={{ left: editRect.left, top: editRect.top, width: editRect.width / view.scale, height: editRect.height / view.scale, transform: `scale(${view.scale})`, transformOrigin: "top left", font: editLayout.font, lineHeight: `${editLayout.lineHeight}px`, color: state.editing.color }} onCompositionStart={() => { composing.current = true; }} onCompositionEnd={() => { composing.current = false; }} onSelect={event => { insertion.current = [event.target.selectionStart, event.target.selectionEnd, event.target.selectionDirection]; }} onChange={event => update({ editing: { ...live.current.editing, text: event.target.value } })} /><div className={`cap-text-frame${gesture.current?.kind === "text" && gesture.current.moved ? " dragging" : ""}`} style={{ left: editRect.left, top: editRect.top, width: editRect.width, height: editRect.height }}>{["top", "right", "bottom", "left"].map(edge => <span key={edge} className={`cap-text-edge ${edge}`} data-text-edge={edge} title={t("拖动边框移动文字", "Drag border to move text")} />)}</div></>}
        {(state.settings.magnifier || state.alt) && state.cursor && (!state.region || state.tool === "select" || state.alt) && !state.editing && <div className="cap-magnifier" style={{ left: clampCapture(state.cursor.x + 20, 8, state.viewport.width - 138), top: clampCapture(state.cursor.y + 20, 8, state.viewport.height - 155) }}><canvas ref={magnifier} width="128" height="96" /><p>{state.cursor.source.x}, {state.cursor.source.y}<br />{state.rgb ? state.cursor.rgb : state.cursor.color.toUpperCase()} · C {t("复制色值", "copy color")}</p></div>}
        {state.preview && state.cursor && <div className="cap-size-preview" style={{ left: clampCapture(state.cursor.x + 16, 8, state.viewport.width - 145), top: clampCapture(state.cursor.y + 20, 8, state.viewport.height - 48) }}><i style={{ width: Math.min(size, 28), height: Math.min(size, 28), background: state.color }} /><span>{t(range.zh, range.en)} {size} px</span></div>}
        {state.error && <div className="cap-error" role="alert"><span>{state.error}</span><button type="button" onClick={() => update({ error: "" })}>{t("关闭提示", "Dismiss")}</button>{!state.base && <button type="button" onClick={() => void close()}>{t("退出截图", "Cancel capture")}</button>}</div>}
      </div>
    </div>
  </section>;
}
