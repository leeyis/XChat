import { useEffect, useRef, useState } from "react";
import {
  createTextOperation,
  drawCaptureOperation,
} from "./capture-drawing.js";
import "./capture-editor.css";

const TOOLS = [
  ["rectangle", "矩形", "Rectangle"],
  ["ellipse", "椭圆", "Ellipse"],
  ["arrow", "箭头", "Arrow"],
  ["pen", "画笔", "Pen"],
  ["mosaic", "马赛克", "Mosaic"],
  ["text", "文本", "Text"],
];

function pointOnCanvas(event, canvas) {
  const bounds = canvas.getBoundingClientRect();
  return {
    x: ((event.clientX - bounds.left) / bounds.width) * canvas.width,
    y: ((event.clientY - bounds.top) / bounds.height) * canvas.height,
  };
}

function closeWindow() {
  const current = globalThis.__TAURI__?.window?.getCurrentWindow?.();
  if (current) current.close();
  else globalThis.close();
}

function CapturePin({ workspace, english }) {
  const [pending, setPending] = useState(null);
  useEffect(() => {
    let disposed = false;
    let unlisten = () => {};
    const load = () => workspace.dispatch({ type: "capture.pending" }).then((result) => {
      if (disposed) return;
      if (result.ok) setPending(result.data);
    });
    load();
    globalThis.__TAURI__?.event
      ?.listen("capture-pin-updated", load)
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten();
    };
  }, [workspace]);
  return (
    <main
      className="capture-pin"
      onMouseDown={(event) => {
        if (event.target.closest("button")) return;
        globalThis.__TAURI__?.window?.getCurrentWindow?.().startDragging?.();
      }}
    >
      {pending?.data_url ? (
        <img src={pending.data_url} alt={pending.file_name || "Capture"} />
      ) : (
        <span>{english ? "Loading capture…" : "正在加载截图…"}</span>
      )}
      <button type="button" onClick={closeWindow} aria-label={english ? "Close" : "关闭"}>
        ×
      </button>
    </main>
  );
}

export default function CaptureEditor({ workspace, mode = "editor" }) {
  const english = document.documentElement.lang.toLowerCase().startsWith("en");
  const [pending, setPending] = useState(null);
  const [image, setImage] = useState(null);
  const [tool, setTool] = useState("rectangle");
  const [color, setColor] = useState("#ff3b30");
  const [size, setSize] = useState(4);
  const [operations, setOperations] = useState([]);
  const [draft, setDraft] = useState(null);
  const [textInput, setTextInput] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const canvas = useRef(null);
  const mosaic = useRef(null);
  const draftRef = useRef(null);
  const operationsRef = useRef([]);
  const textInputRef = useRef(null);

  const replaceOperations = (change) => {
    const next =
      typeof change === "function" ? change(operationsRef.current) : change;
    operationsRef.current = next;
    setOperations(next);
    return next;
  };

  const changeTextInput = (value) => {
    textInputRef.current = value;
    setTextInput(value);
  };

  const commitText = () => {
    const current = textInputRef.current;
    changeTextInput(null);
    const operation = createTextOperation(current, color, size);
    if (!operation) return null;
    replaceOperations((items) => [...items, operation]);
    return operation;
  };

  useEffect(() => {
    workspace.dispatch({ type: "capture.pending" }).then((result) => {
      if (!result.ok || !result.data?.data_url) {
        setError(
          result.error?.message ||
            (english ? "The pending capture is unavailable." : "待编辑截图不可用。"),
        );
        return;
      }
      setPending(result.data);
      const source = new Image();
      source.onload = () => setImage(source);
      source.onerror = () =>
        setError(english ? "Unable to load the capture." : "无法加载截图。");
      source.src = result.data.data_url;
    });
  }, [english, workspace]);

  useEffect(() => {
    if (!image || !canvas.current) return;
    const target = canvas.current;
    target.width = image.naturalWidth;
    target.height = image.naturalHeight;
    const small = document.createElement("canvas");
    small.width = Math.max(1, Math.ceil(target.width / 14));
    small.height = Math.max(1, Math.ceil(target.height / 14));
    small.getContext("2d").drawImage(image, 0, 0, small.width, small.height);
    const pixelated = document.createElement("canvas");
    pixelated.width = target.width;
    pixelated.height = target.height;
    const pixelContext = pixelated.getContext("2d");
    pixelContext.imageSmoothingEnabled = false;
    pixelContext.drawImage(small, 0, 0, target.width, target.height);
    mosaic.current = pixelated;
  }, [image]);

  useEffect(() => {
    if (!image || !canvas.current || !mosaic.current) return;
    const context = canvas.current.getContext("2d");
    context.clearRect(0, 0, canvas.current.width, canvas.current.height);
    context.drawImage(image, 0, 0);
    for (const operation of operations) {
      drawCaptureOperation(context, operation, mosaic.current);
    }
    if (draft) drawCaptureOperation(context, draft, mosaic.current);
  }, [draft, image, operations]);

  useEffect(() => {
    const shortcut = (event) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "z") {
        event.preventDefault();
        replaceOperations((current) => current.slice(0, -1));
      }
    };
    addEventListener("keydown", shortcut);
    return () => removeEventListener("keydown", shortcut);
  }, []);

  if (mode === "pin") return <CapturePin workspace={workspace} english={english} />;

  const changeDraft = (value) => {
    draftRef.current = value;
    setDraft(value);
  };

  const begin = (event) => {
    if (!image || busy || event.button !== 0) return;
    const point = pointOnCanvas(event, canvas.current);
    if (tool === "text") {
      changeTextInput({ ...point, value: "" });
      return;
    }
    canvas.current.setPointerCapture(event.pointerId);
    changeDraft(
      tool === "pen" || tool === "mosaic"
        ? { tool, color, size, points: [point] }
        : { tool, color, size, start: point, end: point },
    );
  };

  const move = (event) => {
    const current = draftRef.current;
    if (!current) return;
    const point = pointOnCanvas(event, canvas.current);
    changeDraft(
      current.points
        ? { ...current, points: [...current.points, point] }
        : { ...current, end: point },
    );
  };

  const end = () => {
    const current = draftRef.current;
    if (!current) return;
    const useful = current.points
      ? current.points.length > 1
      : Math.hypot(
          current.end.x - current.start.x,
          current.end.y - current.start.y,
        ) > 2;
    if (useful) replaceOperations((items) => [...items, current]);
    changeDraft(null);
  };

  const exportPng = () => {
    commitText();
    const context = canvas.current.getContext("2d");
    context.clearRect(0, 0, canvas.current.width, canvas.current.height);
    context.drawImage(image, 0, 0);
    for (const operation of operationsRef.current) {
      drawCaptureOperation(context, operation, mosaic.current);
    }
    return canvas.current.toDataURL("image/png");
  };
  const finish = async () => {
    setBusy(true);
    const dataUrl = exportPng();
    const result = await workspace.dispatch({
      type: "capture.finish",
      dataUrl,
    });
    setBusy(false);
    if (!result.ok) {
      setError(result.error.message);
      return;
    }
    if (globalThis.BroadcastChannel) {
      const channel = new BroadcastChannel("xchat-capture");
      channel.postMessage({
        type: "capture-ready",
        attachment: { ...result.data, preview_url: dataUrl },
      });
      channel.close();
    }
    closeWindow();
  };

  const cancel = async () => {
    await workspace.dispatch({ type: "capture.cancel" });
    closeWindow();
  };

  const pin = async () => {
    setBusy(true);
    const result = await workspace.dispatch({
      type: "capture.pin",
      dataUrl: exportPng(),
    });
    setBusy(false);
    if (result.ok) closeWindow();
    else setError(result.error.message);
  };

  return (
    <main className="capture-editor">
      <header className="capture-editor-titlebar" data-tauri-drag-region>
        <b>{english ? "Capture editor" : "截图编辑"}</b>
        <span>{pending?.file_name || ""}</span>
      </header>
      <section className="capture-stage">
        {error && <div className="capture-error">{error}</div>}
        {!image && !error && (
          <div className="capture-loading">
            {english ? "Loading capture…" : "正在加载截图…"}
          </div>
        )}
        <div className="capture-canvas-wrap">
          <canvas
            ref={canvas}
            hidden={!image}
            onPointerDown={begin}
            onPointerMove={move}
            onPointerUp={end}
            onPointerCancel={end}
          />
          {textInput && image && (
            <input
              className="capture-text-input"
              style={{
                left: `${(textInput.x / image.naturalWidth) * 100}%`,
                top: `${(textInput.y / image.naturalHeight) * 100}%`,
                color,
                fontSize: `${Math.max(16, size * 5)}px`,
              }}
              value={textInput.value}
              autoFocus
              onChange={(event) =>
                changeTextInput({
                  ...textInputRef.current,
                  value: event.target.value,
                })
              }
              onBlur={commitText}
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  changeTextInput(null);
                }
                if (event.key === "Enter") {
                  event.preventDefault();
                  commitText();
                }
              }}
            />
          )}
        </div>
      </section>
      <footer className="capture-toolbar">
        <div className="capture-tools">
          {TOOLS.map(([value, zh, en]) => (
            <button
              type="button"
              className={tool === value ? "active" : ""}
              onClick={() => setTool(value)}
              key={value}
            >
              {english ? en : zh}
            </button>
          ))}
          <label>
            <span>{english ? "Color" : "颜色"}</span>
            <input
              type="color"
              value={color}
              onChange={(event) => setColor(event.target.value)}
            />
          </label>
          <label>
            <span>{english ? "Size" : "粗细"}</span>
            <input
              type="range"
              min="2"
              max="16"
              value={size}
              onChange={(event) => setSize(Number(event.target.value))}
            />
          </label>
          <button
            type="button"
            disabled={!operations.length}
            onClick={() => replaceOperations((current) => current.slice(0, -1))}
          >
            {english ? "Undo" : "回退"}
          </button>
        </div>
        <div className="capture-actions">
          <button type="button" onClick={cancel} disabled={busy}>
            {english ? "Cancel" : "取消"}
          </button>
          <button type="button" onClick={pin} disabled={!image || busy}>
            {english ? "Pin" : "钉图"}
          </button>
          <button
            type="button"
            className="primary"
            onClick={finish}
            disabled={!image || busy}
          >
            {english ? "Done" : "完成"}
          </button>
        </div>
      </footer>
    </main>
  );
}
