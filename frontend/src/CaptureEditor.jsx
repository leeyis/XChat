import { useEffect, useRef, useState } from "react";
import CaptureSurface from "./CaptureSurface.jsx";
import CapturePin, { captureDispatch, expandPinOverlay } from "./CapturePin.jsx";
import { captureRecord, patchCaptureRecord, saveCaptureRecord, watchCaptureLibrary } from "./capture-library.js";
import { normalizePinView } from "./capture-model.js";
import "./capture-widgets.css";

export default function CaptureEditor({ workspace, mode = "editor" }) {
  const english = document.documentElement.lang.toLowerCase().startsWith("en");
  const [record, setRecord] = useState(null), [pending, setPending] = useState(null), [editing, setEditing] = useState(null), [error, setError] = useState("");
  const latest = useRef(null), exiting = useRef(false), active = useRef(true), initialRequest = useRef(null);
  latest.current = record;
  useEffect(() => {
    active.current = true;
    let initial = initialRequest.current;
    if (!initial || initial.workspace !== workspace || initial.mode !== mode) {
      initial = { workspace, mode, promise: null };
      initialRequest.current = initial;
    }
    if (mode !== "pin") return undefined;
    const elements = [document.documentElement, document.body, document.getElementById("root")].filter(Boolean);
    elements.forEach(element => element.classList.add("capture-view-transparent"));
    let disposed = false;
    let nativeLoadSequence = 0, recordLoadSequence = 0, viewRevision = 0;
    const stops = [];
    const load = async (firstLoad = false) => {
      if (disposed) return;
      const request = ++nativeLoadSequence, initialViewRevision = viewRevision;
      try {
        // Only the initial request is shared across this mount's StrictMode replay.
        const source = await (firstLoad
          ? (initial.promise ||= captureDispatch(workspace, { type: "capture.pending" }))
          : captureDispatch(workspace, { type: "capture.pending" }));
        if (disposed || request !== nativeLoadSequence) return;
        const id = source.pin_id || source.session_id;
        const saved = await captureRecord(id);
        if (disposed || request !== nativeLoadSequence) return;
        const view = initialViewRevision !== viewRevision && latest.current?.id === id ? latest.current.view : source.view || saved?.view;
        const next = { ...saved, id, kind: "pin", data_url: source.data_url, width: source.width, height: source.height, file_name: source.file_name, document: saved?.document || source.document, view: normalizePinView(view) };
        if (!saved) await saveCaptureRecord(next);
        else if (JSON.stringify(normalizePinView(saved.view)) !== JSON.stringify(next.view)) await patchCaptureRecord(id, { view: next.view });
        if (disposed || request !== nativeLoadSequence) return;
        setPending(source); setRecord(next); setError("");
      } catch (cause) { if (!disposed && request === nativeLoadSequence) setError(cause.message); }
    };
    const refreshRecord = async () => {
      const id = latest.current?.id, request = ++recordLoadSequence;
      if (!id) return;
      try {
        const saved = await captureRecord(id);
        if (disposed || request !== recordLoadSequence || !saved || latest.current?.id !== id) return;
        // Native events are authoritative for the live window. A debounced
        // database update can still contain an earlier drag position.
        const next = { ...latest.current, ...saved, view: latest.current.view };
        latest.current = next; setRecord(next);
      } catch (cause) { if (!disposed) setError(cause.message); }
    };
    void load(true);
    stops.push(watchCaptureLibrary(change => { if (change.id === latest.current?.id && change.kind !== "delete") void refreshRecord(); }));
    const events = globalThis.__TAURI__?.event;
    if (events?.listen) {
      events.listen("capture-pin-updated", event => { if (!event.payload?.pin_id || event.payload.pin_id === latest.current?.id) void load(); }).then(stop => disposed ? stop() : stops.push(stop)).catch(cause => setError(cause.message));
      events.listen("capture-pin-view-updated", event => {
        const value = event.payload;
        if (value?.pin_id !== latest.current?.id) return;
        viewRevision += 1;
        const next = { ...latest.current, view: normalizePinView(value.view) };
        latest.current = next;
        setRecord(next);
      }).then(stop => disposed ? stop() : stops.push(stop)).catch(cause => setError(cause.message));
    }
    return () => { disposed = true; active.current = false; stops.forEach(stop => stop()); elements.forEach(element => element.classList.remove("capture-view-transparent")); };
  }, [workspace, mode]);

  async function enterEdit(item) {
    try {
      await expandPinOverlay(workspace, item, true);
      setEditing({ ...pending, data_url: item.data_url, width: item.width, height: item.height, pin_id: item.id, session_id: item.id, document: item.document, view: item.view });
    } catch (cause) { setError(cause.message); }
  }
  async function exitEdit() {
    if (exiting.current) return;
    exiting.current = true;
    try {
      const current = latest.current;
      const saved = await captureRecord(current.id);
      const source = await captureDispatch(workspace, { type: "capture.pending" });
      const next = { ...current, ...saved, data_url: source.data_url, width: source.width, height: source.height, view: normalizePinView(saved?.view || current.view) };
      await captureDispatch(workspace, { type: "capture.pin.update", pinId: next.id, view: next.view, overlay: false });
      if (active.current) { setRecord(next); setPending(source); setEditing(null); }
    } catch (cause) { if (active.current) setError(cause.message); }
    finally { exiting.current = false; }
  }

  if (mode !== "pin") return <CaptureSurface workspace={workspace} english={english} />;
  if (editing) return <CaptureSurface workspace={workspace} english={english} pending={editing} onClose={exitEdit} />;
  return <main className="cap-root cap-native-root" aria-label={english ? "Pinned image" : "贴图"}>
    {record ? <CapturePin workspace={workspace} record={record} native english={english} onEdit={enterEdit} onChange={setRecord} /> : <span className="cap-native-loading">{english ? "Loading image…" : "正在加载贴图…"}</span>}
    {error && <div className="cap-pin-feedback" role="alert">{error}</div>}
  </main>;
}
