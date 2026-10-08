import { useCallback, useEffect, useRef, useState } from "react";
import CaptureSurface from "./CaptureSurface.jsx";
import CapturePin, { captureDispatch } from "./CapturePin.jsx";
import { captureRecord } from "./capture-library.js";
import { normalizePinView } from "./capture-model.js";

export default function CapturePinOverlay({ workspace, mode, english }) {
  const [source, setSource] = useState(null), [error, setError] = useState("");
  const [revision, setRevision] = useState(0);
  const request = useRef(null), shown = useRef(false);
  const close = useCallback(() => captureDispatch(workspace, { type: "capture.pin.overlay.close" }).catch(cause => setError(cause.message)), [workspace]);
  const ready = useCallback(() => {
    if (shown.current) return;
    shown.current = true;
    void captureDispatch(workspace, { type: "capture.pin.overlay.ready" }).catch(cause => { setError(cause.message); void close(); });
  }, [workspace, close, revision]);
  useEffect(() => {
    let disposed = false;
    const elements = [document.documentElement, document.body, document.getElementById("root")].filter(Boolean);
    elements.forEach(element => element.classList.add("capture-view-transparent"));
    const stops = [];
    if (mode === "menu") {
      globalThis.__TAURI__.event.listen("capture-pin-menu-open", event => {
        if (disposed) return;
        shown.current = false;
        setSource(current => current ? { ...current, ...event.payload, view: normalizePinView(event.payload.view) } : current);
        setRevision(value => value + 1);
      }).then(stop => disposed ? stop() : stops.push(stop)).catch(cause => setError(cause.message));
    }
    request.current ||= captureDispatch(workspace, { type: "capture.pending" }).then(async pending => {
      const saved = mode === "menu" ? null : await captureRecord(pending.pin_id);
      return { ...saved, ...pending, id: pending.pin_id, document: saved?.document || pending.document, view: normalizePinView(pending.view || saved?.view) };
    });
    request.current.then(value => { if (!disposed) setSource(value); }).catch(cause => {
      if (!disposed) { setError(cause.message); void close(); }
    });
    return () => { disposed = true; stops.forEach(stop => stop()); elements.forEach(element => element.classList.remove("capture-view-transparent")); };
  }, [workspace, close, mode]);
  if (!source) return null;
  return <>
    {mode === "menu" ? <CapturePin workspace={workspace} record={source} native menuOnly english={english} onReady={ready} /> : <CaptureSurface workspace={workspace} pending={source} english={english} onClose={close} onReady={ready} />}
    {error && <div className="cap-pin-feedback" role="alert">{error}</div>}
  </>;
}
