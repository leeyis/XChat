import { useEffect, useRef, useState } from "react";
import CaptureSurface from "./CaptureSurface.jsx";
import CapturePin, { captureDispatch } from "./CapturePin.jsx";
import { captureRecords, captureSettings, patchCaptureRecord, watchCaptureLibrary } from "./capture-library.js";
import { normalizePinView } from "./capture-model.js";
import "./capture-widgets.css";

const nativeCapture = () => Boolean(globalThis.__TAURI__?.core?.invoke || globalThis.__TAURI_INTERNALS__?.invoke);

// Saved images live independently of the capture tools; starting capture opens
// the selection directly, without a landing page.
export default function CaptureWorkspace({ workspace, english = false }) {
  const t = (zh, en) => english ? en : zh;
  const native = nativeCapture();
  const [records, setRecords] = useState([]), [settings, setSettings] = useState(captureSettings);
  const [pending, setPending] = useState(null), [toast, setToast] = useState(null);
  const [deferredPins, setDeferredPins] = useState(() => new Set()), [countdown, setCountdown] = useState(0);
  const live = useRef(null), restored = useRef(false), recovering = useRef(false), countdownSession = useRef(null);
  live.current = { pending, countdown };
  function notify(message, error = false, restoreId = null) { setToast({ message, error, restoreId }); }
  function revealPin(id) { setDeferredPins(current => { const next = new Set(current); next.delete(id); return next; }); }
  async function run(action) {
    try {
      const result = await captureDispatch(workspace, action);
      if (action.type === "capture.pin.restore") revealPin(action.pinId);
      return result;
    } catch (cause) { notify(cause.message, true); return null; }
  }
  async function load() {
    const items = await captureRecords();
    setRecords(items); setSettings(captureSettings());
    return items;
  }
  async function recover() {
    if (recovering.current || live.current.pending) return;
    recovering.current = true;
    try {
      const items = await captureRecords(), group = captureSettings().group || "默认";
      const existing = native ? new Set((await captureDispatch(workspace, { type: "capture.pin.list" })).map(pin => pin.pin_id || pin.session_id)) : new Set();
      for (const item of items.filter(item => item.kind === "pin" && normalizePinView(item.view).group === group)) {
        // Native F3 already restored live windows; do not overwrite an active edit.
        if (existing.has(item.id)) continue;
        await captureDispatch(workspace, { type: "capture.pin.restore", pinId: item.id });
        revealPin(item.id);
      }
      await load();
    } catch (cause) { notify(cause.message, true); }
    finally { recovering.current = false; }
  }
  useEffect(() => {
    let disposed = false;
    const viewWrites = new Map(), stops = [];
    const refresh = async () => { try { if (!disposed) await load(); } catch (cause) { if (!disposed) notify(cause.message, true); } };
    void (async () => {
      try {
        const preferences = await captureDispatch(workspace, { type: "capture.preferences", delaySeconds: captureSettings().delay });
        if (disposed) return;
        if (native && preferences?.cursorSupported) await captureDispatch(workspace, { type: "capture.preferences", captureCursor: captureSettings().captureCursor });
        const items = await captureRecords();
        if (disposed) return;
        setRecords(items);
        if (!captureSettings().remember) setDeferredPins(new Set(items.filter(item => item.kind === "pin").map(item => item.id)));
        if (native && captureSettings().remember && !restored.current) {
          restored.current = true;
          await captureDispatch(workspace, { type: "capture.group", group: captureSettings().group || "默认" });
          for (const item of items.filter(item => item.kind === "pin" && !item.view?.hidden)) {
            if (disposed) break;
            await captureDispatch(workspace, { type: "capture.pin.restore", pinId: item.id });
          }
        }
      } catch (cause) { if (!disposed) notify(cause.message, true); }
    })();
    stops.push(watchCaptureLibrary(refresh));
    const openCapture = event => { if (event.detail?.data_url) setPending(event.detail); };
    const onCountdown = event => {
      const value = event.detail || event.payload || {}, remaining = Math.max(0, Number(value.remaining) || 0);
      if (!remaining && value.session_id && countdownSession.current && countdownSession.current !== value.session_id) return;
      countdownSession.current = remaining ? value.session_id || null : null;
      setCountdown(remaining);
    };
    const keyboard = event => {
      if (event.key === "Escape" && live.current.countdown > 0) {
        event.preventDefault(); event.stopPropagation();
        void run({ type: "capture.cancel-start", sessionId: countdownSession.current });
      } else if (event.key === "F3" && !native && !event.isComposing && !live.current.pending) {
        event.preventDefault(); void recover();
      }
    };
    addEventListener("xchat-capture-open", openCapture); addEventListener("xchat-capture-countdown", onCountdown); addEventListener("keydown", keyboard);
    stops.push(() => { removeEventListener("xchat-capture-open", openCapture); removeEventListener("xchat-capture-countdown", onCountdown); removeEventListener("keydown", keyboard); });
    const events = globalThis.__TAURI__?.event;
    if (events?.listen) {
      const listen = (name, handler) => events.listen(name, handler).then(stop => disposed ? stop() : stops.push(stop)).catch(cause => { if (!disposed) notify(cause.message, true); });
      void listen("capture-pin-recover", () => void recover());
      void listen("capture-countdown", onCountdown);
      void listen("capture-pin-view-updated", event => {
        const value = event.payload;
        if (!value?.pin_id || !value.view) return;
        const view = normalizePinView(value.view);
        setRecords(current => current.map(item => item.id === value.pin_id ? { ...item, view } : item));
        clearTimeout(viewWrites.get(value.pin_id)?.timer);
        const timer = setTimeout(async () => {
          viewWrites.delete(value.pin_id);
          try { await patchCaptureRecord(value.pin_id, { view }); }
          catch (cause) { if (!disposed) notify(cause.message, true); }
        }, 150);
        viewWrites.set(value.pin_id, { timer, view });
      });
    }
    return () => {
      disposed = true; stops.forEach(stop => stop());
      viewWrites.forEach(({ timer, view }, id) => { clearTimeout(timer); void patchCaptureRecord(id, { view }).catch(() => {}); });
    };
  }, [workspace, native]);
  useEffect(() => { if (!toast) return undefined; const timer = setTimeout(() => setToast(null), toast.restoreId ? 8000 : 5000); return () => clearTimeout(timer); }, [toast]);
  function edit(item) {
    const view = normalizePinView(item.view);
    setPending({ session_id: item.id, pin_id: item.id, data_url: item.data_url, width: item.width, height: item.height, document: item.document, conversation_id: item.conversation_id ?? null, view, edit_viewport: { x: view.x, y: view.y, pixelRatio: 1 } });
  }
  const visiblePins = records.filter(item => item.kind === "pin" && !item.view?.hidden && !deferredPins.has(item.id) && normalizePinView(item.view).group === (settings.group || "默认"));
  return <>
    {!native && <section className="cap-root cap-pin-layer" aria-label={t("页面贴图", "Pinned images")}>{[...visiblePins].reverse().filter(item => item.id !== pending?.pin_id).map(item => <CapturePin key={item.id} workspace={workspace} record={item} english={english} onEdit={edit} onHidden={(value, destroyed) => { if (!destroyed) notify(t("贴图已隐藏，按 F3 原位恢复", "Image hidden. Press F3 to restore it in place."), false, value.id); }} />)}</section>}
    {pending && <CaptureSurface key={pending.session_id} workspace={workspace} pending={pending} english={english} onClose={() => { setPending(current => current?.session_id === pending.session_id ? null : current); void load().catch(cause => notify(cause.message, true)); }} />}
    {countdown > 0 && <section className="cap-root cap-countdown-host" aria-label={t("截图倒计时", "Capture countdown")}><div className="cap-countdown"><strong aria-live="polite">{countdown}</strong><button type="button" onClick={() => void run({ type: "capture.cancel-start", sessionId: countdownSession.current })}>{t("取消", "Cancel")}</button></div></section>}
    {toast && <section className="cap-root cap-workspace-toast"><div className={`cap-toast show${toast.error ? " error" : ""}`} role={toast.error ? "alert" : "status"}><span>{toast.message}</span>{toast.restoreId && <button type="button" onClick={() => { void run({ type: "capture.pin.restore", pinId: toast.restoreId }); setToast(null); }}>{t("恢复", "Restore")}</button>}</div></section>}
  </>;
}
