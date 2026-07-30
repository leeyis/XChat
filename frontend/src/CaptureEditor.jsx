import { useEffect, useRef, useState } from "react";
import {
  addCaptureOperation,
  createCaptureHistory,
  createTextOperation,
  drawCaptureOperation,
  fitPinnedCapture,
  moveCaptureSelection,
  nextPinnedCaptureZoom,
  normalizeCaptureSelection,
  placeCaptureTextInput,
  placePinnedCaptureMenu,
  placeCaptureToolbar,
  redoCaptureOperation,
  resizeCaptureSelection,
  undoCaptureOperation,
} from "./capture-drawing.js";
import "./capture-editor.css";

const TOOLS = [
  { value: "rectangle", zh: "矩形", en: "Rectangle" },
  { value: "ellipse", zh: "椭圆", en: "Ellipse" },
  { value: "arrow", zh: "箭头", en: "Arrow" },
  { value: "pen", zh: "画笔", en: "Pen" },
  { value: "mosaic", zh: "马赛克", en: "Mosaic" },
  { value: "text", zh: "文本", en: "Text" },
];
const COLORS = ["#f04444", "#f59e0b", "#16a66a", "#1687f8", "#111827", "#ffffff"];
const SIZES = [2, 4, 8];
const HANDLES = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];
const PIN_MENU_SIZE = { width: 260, height: 360 };

function pointOnCanvas(event, canvas) {
  const bounds = canvas.getBoundingClientRect();
  return {
    x: ((event.clientX - bounds.left) / bounds.width) * canvas.width,
    y: ((event.clientY - bounds.top) / bounds.height) * canvas.height,
  };
}

function pointInViewport(event, element) {
  const bounds = element.getBoundingClientRect();
  return {
    x: event.clientX - bounds.left,
    y: event.clientY - bounds.top,
  };
}

function mosaicForCanvas(source) {
  const small = document.createElement("canvas");
  small.width = Math.max(1, Math.ceil(source.width / 14));
  small.height = Math.max(1, Math.ceil(source.height / 14));
  small.getContext("2d").drawImage(source, 0, 0, small.width, small.height);
  const pixelated = document.createElement("canvas");
  pixelated.width = source.width;
  pixelated.height = source.height;
  const context = pixelated.getContext("2d");
  context.imageSmoothingEnabled = false;
  context.drawImage(small, 0, 0, source.width, source.height);
  return pixelated;
}

function closeWindow() {
  const current = globalThis.__TAURI__?.window?.getCurrentWindow?.();
  if (current) current.close();
  else globalThis.close();
}

function CaptureIcon({ name }) {
  const common = {
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.8,
    strokeLinecap: "round",
    strokeLinejoin: "round",
  };
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
      {name === "rectangle" && <rect {...common} x="4" y="5" width="16" height="14" rx="1" />}
      {name === "ellipse" && <ellipse {...common} cx="12" cy="12" rx="8" ry="6.5" />}
      {name === "arrow" && (
        <>
          <path {...common} d="M4 18 19 5" />
          <path {...common} d="m12 5 7 0 0 7" />
        </>
      )}
      {name === "pen" && (
        <>
          <path {...common} d="m4 20 4.2-1 10.6-10.6-3.2-3.2L5 15.8 4 20Z" />
          <path {...common} d="m13.8 7 3.2 3.2" />
        </>
      )}
      {name === "mosaic" && (
        <>
          <rect {...common} x="4" y="4" width="6" height="6" />
          <rect {...common} x="14" y="4" width="6" height="6" />
          <rect {...common} x="4" y="14" width="6" height="6" />
          <rect {...common} x="14" y="14" width="6" height="6" />
        </>
      )}
      {name === "text" && (
        <>
          <path {...common} d="M5 5h14" />
          <path {...common} d="M12 5v14" />
          <path {...common} d="M8 19h8" />
        </>
      )}
      {name === "undo" && (
        <>
          <path {...common} d="m9 7-5 4 5 4" />
          <path {...common} d="M5 11h7a7 7 0 0 1 7 7" />
        </>
      )}
      {name === "redo" && (
        <>
          <path {...common} d="m15 7 5 4-5 4" />
          <path {...common} d="M19 11h-7a7 7 0 0 0-7 7" />
        </>
      )}
      {name === "cancel" && (
        <>
          <path {...common} d="m6 6 12 12" />
          <path {...common} d="m18 6-12 12" />
        </>
      )}
      {name === "pin" && (
        <>
          <path {...common} d="m9 4 6 0-1 5 3 3H7l3-3-1-5Z" />
          <path {...common} d="M12 12v8" />
        </>
      )}
      {name === "save" && (
        <>
          <path {...common} d="M5 4h12l2 2v14H5V4Z" />
          <path {...common} d="M8 4v6h8V4" />
          <path {...common} d="M8 20v-6h8v6" />
        </>
      )}
      {name === "done" && <path {...common} d="m5 12 4 4L19 6" />}
    </svg>
  );
}

function CapturePin({ workspace, english }) {
  const [pending, setPending] = useState(null);
  const [zoom, setZoom] = useState(1);
  const [menu, setMenu] = useState(null);
  const [toolbarVisible, setToolbarVisible] = useState(false);
  const [shadow, setShadow] = useState(true);
  const [status, setStatus] = useState("");

  const runPinAction = async (action, successMessage = "") => {
    setMenu(null);
    setStatus("");
    const result = await workspace.dispatch(action);
    if (result.ok) {
      if (successMessage && result.data !== null) setStatus(successMessage);
      return result.data;
    }
    setStatus(result.error.message);
    return null;
  };

  const applyZoom = async (next) => {
    if (!pending || next === zoom) return;
    setMenu(null);
    const result = await workspace.dispatch({
      type: "capture.pin.resize",
      scale: next,
    });
    if (result.ok) setZoom(Number(result.data ?? next));
    else setStatus(result.error.message);
  };

  const hidePin = () => runPinAction({ type: "capture.pin.close", destroy: false });
  const destroyPin = () => runPinAction({ type: "capture.pin.close", destroy: true });

  useEffect(() => {
    let disposed = false;
    let unlisten = () => {};
    const load = () => workspace.dispatch({ type: "capture.pending" }).then((result) => {
      if (disposed) return;
      if (result.ok) {
        setPending(result.data);
        setZoom(fitPinnedCapture(result.data.width, result.data.height));
      }
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

  useEffect(() => {
    const dismiss = () => setMenu(null);
    const escape = (event) => {
      if (event.key === "Escape") dismiss();
    };
    addEventListener("blur", dismiss);
    addEventListener("resize", dismiss);
    addEventListener("keydown", escape);
    return () => {
      removeEventListener("blur", dismiss);
      removeEventListener("resize", dismiss);
      removeEventListener("keydown", escape);
    };
  }, []);

  const copyPin = (original) =>
    runPinAction(
      {
        type: "capture.pin.copy",
        scale: original ? null : zoom,
      },
      english ? "Copied" : "已复制",
    );

  const savePin = () =>
    runPinAction(
      { type: "capture.pin.save" },
      english ? "Saved" : "已保存",
    );

  const toggleShadow = async () => {
    const next = !shadow;
    setShadow(next);
    await runPinAction({ type: "capture.pin.shadow", enabled: next });
  };

  return (
    <main
      className={`capture-pin${shadow ? " with-shadow" : ""}`}
      onMouseDown={(event) => {
        if (event.button !== 0 || event.target.closest("button, [role='menu']")) return;
        setMenu(null);
        globalThis.__TAURI__?.window?.getCurrentWindow?.().startDragging?.();
      }}
      onWheel={(event) => {
        event.preventDefault();
        const next = nextPinnedCaptureZoom(zoom, event.deltaY);
        void applyZoom(next);
      }}
      onContextMenu={(event) => {
        event.preventDefault();
        event.stopPropagation();
        setMenu(
          placePinnedCaptureMenu(
            { x: event.clientX, y: event.clientY },
            PIN_MENU_SIZE,
            { width: globalThis.innerWidth, height: globalThis.innerHeight },
          ),
        );
      }}
    >
      {pending?.data_url ? (
        <img
          src={pending.data_url}
          alt={pending.file_name || "Capture"}
          draggable="false"
        />
      ) : (
        <span>{english ? "Loading capture…" : "正在加载截图…"}</span>
      )}
      {toolbarVisible && (
        <div className="capture-pin-toolbar" role="toolbar">
          <button
            type="button"
            onClick={() => void applyZoom(nextPinnedCaptureZoom(zoom, 120))}
            aria-label={english ? "Zoom out" : "缩小"}
          >
            −
          </button>
          <button
            type="button"
            className="zoom-value"
            onClick={() => void applyZoom(1)}
            title={english ? "Original size" : "原始大小"}
          >
            {Math.round(zoom * 100)}%
          </button>
          <button
            type="button"
            onClick={() => void applyZoom(nextPinnedCaptureZoom(zoom, -120))}
            aria-label={english ? "Zoom in" : "放大"}
          >
            +
          </button>
          <span aria-hidden="true" />
          <button type="button" onClick={() => void copyPin(true)}>
            {english ? "Copy" : "复制"}
          </button>
          <button type="button" onClick={() => void savePin()}>
            {english ? "Save" : "保存"}
          </button>
          <button type="button" onClick={() => void hidePin()} aria-label={english ? "Close" : "关闭"}>
            ×
          </button>
        </div>
      )}
      {!toolbarVisible && (
        <button
          type="button"
          className="capture-pin-close"
          onClick={() => void hidePin()}
          aria-label={english ? "Close" : "关闭"}
        >
          ×
        </button>
      )}
      {menu && (
        <div
          className="capture-pin-menu"
          style={{ left: menu.left, top: menu.top }}
          role="menu"
          onMouseDown={(event) => event.stopPropagation()}
          onWheel={(event) => event.stopPropagation()}
        >
          <button type="button" role="menuitem" onClick={() => void copyPin(false)}>
            {english ? "Copy Image" : "复制图像"}
          </button>
          <button type="button" role="menuitem" onClick={() => void copyPin(true)}>
            {english ? "Copy Original Size Image" : "复制原始大小图像"}
          </button>
          <button type="button" role="menuitem" onClick={() => void savePin()}>
            {english ? "Save Image As…" : "图像另存为…"}
          </button>
          <span className="menu-separator" aria-hidden="true" />
          <button type="button" role="menuitem" onClick={() => void applyZoom(1)}>
            {english ? "Original Size (100%)" : "原始大小（100%）"}
            <kbd>⌘0</kbd>
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={() =>
              void applyZoom(fitPinnedCapture(pending?.width, pending?.height))
            }
          >
            {english ? "Fit to Window" : "适合窗口"}
          </button>
          <span className="menu-separator" aria-hidden="true" />
          <button
            type="button"
            role="menuitemcheckbox"
            aria-checked={toolbarVisible}
            onClick={() => {
              setToolbarVisible((current) => !current);
              setMenu(null);
            }}
          >
            <i>{toolbarVisible ? "✓" : ""}</i>
            {english ? "Show Toolbar" : "显示工具条"}
          </button>
          <button
            type="button"
            role="menuitemcheckbox"
            aria-checked={shadow}
            onClick={() => void toggleShadow()}
          >
            <i>{shadow ? "✓" : ""}</i>
            {english ? "Window Shadow" : "窗口阴影"}
          </button>
          <span className="menu-separator" aria-hidden="true" />
          <button type="button" role="menuitem" onClick={() => void hidePin()}>
            {english ? "Close" : "关闭"}
          </button>
          <button
            type="button"
            className="danger"
            role="menuitem"
            onClick={() => void destroyPin()}
          >
            {english ? "Destroy" : "销毁"}
          </button>
        </div>
      )}
      {status && <div className="capture-pin-status">{status}</div>}
    </main>
  );
}

function CaptureOverlay({ workspace, english }) {
  const [pending, setPending] = useState(null);
  const [image, setImage] = useState(null);
  const [viewport, setViewport] = useState(() => ({
    width: globalThis.innerWidth,
    height: globalThis.innerHeight,
  }));
  const [selection, setSelection] = useState(null);
  const [selectionReady, setSelectionReady] = useState(false);
  const [selectionLocked, setSelectionLocked] = useState(false);
  const [tool, setTool] = useState(null);
  const [color, setColor] = useState(COLORS[0]);
  const [size, setSize] = useState(4);
  const [history, setHistory] = useState(createCaptureHistory);
  const [draft, setDraft] = useState(null);
  const [textInput, setTextInput] = useState(null);
  const [styleOpen, setStyleOpen] = useState(false);
  const [toolbarSize, setToolbarSize] = useState({ width: 520, height: 52 });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const stage = useRef(null);
  const canvas = useRef(null);
  const toolbar = useRef(null);
  const mosaic = useRef(null);
  const selectionRef = useRef(null);
  const historyRef = useRef(history);
  const draftRef = useRef(null);
  const textInputRef = useRef(null);
  const textInputElement = useRef(null);
  const textInputSequence = useRef(0);
  const interactionRef = useRef(null);
  const cancelRef = useRef(null);

  const changeSelection = (value) => {
    selectionRef.current = value;
    setSelection(value);
  };

  const changeHistory = (change) => {
    const next =
      typeof change === "function" ? change(historyRef.current) : change;
    historyRef.current = next;
    setHistory(next);
    return next;
  };

  const changeDraft = (value) => {
    draftRef.current = value;
    setDraft(value);
  };

  const changeTextInput = (value) => {
    textInputRef.current = value;
    setTextInput(value);
  };

  const addOperation = (operation) => {
    changeHistory((current) => addCaptureOperation(current, operation));
    setSelectionLocked(true);
  };

  const commitText = () => {
    const current = textInputRef.current;
    changeTextInput(null);
    const operation = createTextOperation(
      current,
      current?.color ?? color,
      current?.size ?? size,
    );
    if (operation) addOperation(operation);
    return operation;
  };

  useEffect(() => {
    if (!textInput?.id) return undefined;
    const frame = requestAnimationFrame(() => {
      const target = textInputElement.current;
      target?.focus({ preventScroll: true });
      target?.setSelectionRange?.(target.value.length, target.value.length);
    });
    return () => cancelAnimationFrame(frame);
  }, [textInput?.id]);

  const exportPng = () => {
    commitText();
    const current = selectionRef.current;
    if (!image || !current) throw new Error("capture_selection_unavailable");
    const scaleX = image.naturalWidth / viewport.width;
    const scaleY = image.naturalHeight / viewport.height;
    const output = document.createElement("canvas");
    output.width = Math.max(1, Math.round(current.width * scaleX));
    output.height = Math.max(1, Math.round(current.height * scaleY));
    const context = output.getContext("2d");
    context.drawImage(
      image,
      Math.round(current.x * scaleX),
      Math.round(current.y * scaleY),
      output.width,
      output.height,
      0,
      0,
      output.width,
      output.height,
    );
    const pixelated = mosaicForCanvas(output);
    for (const operation of historyRef.current.operations) {
      drawCaptureOperation(context, operation, pixelated);
    }
    return output.toDataURL("image/png");
  };

  const cancel = async () => {
    setBusy(true);
    try {
      await workspace.dispatch({ type: "capture.cancel" });
    } finally {
      closeWindow();
    }
  };
  cancelRef.current = cancel;

  const finish = async () => {
    setBusy(true);
    setError("");
    const dataUrl = exportPng();
    const result = await workspace.dispatch({ type: "capture.finish", dataUrl });
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

  const pin = async () => {
    setBusy(true);
    setError("");
    const result = await workspace.dispatch({
      type: "capture.pin",
      dataUrl: exportPng(),
    });
    setBusy(false);
    if (result.ok) closeWindow();
    else setError(result.error.message);
  };

  const save = async () => {
    setBusy(true);
    setError("");
    setStatus("");
    const result = await workspace.dispatch({
      type: "capture.save",
      dataUrl: exportPng(),
    });
    setBusy(false);
    if (!result.ok) {
      setError(result.error.message);
      return;
    }
    if (result.data?.file_path) {
      setStatus(english ? "Saved" : "已保存");
    }
  };

  useEffect(() => {
    let disposed = false;
    workspace.dispatch({ type: "capture.pending" }).then(async (result) => {
      if (disposed) return;
      if (!result.ok || !result.data?.data_url) {
        setError(
          result.error?.message ||
            (english ? "The pending capture is unavailable." : "待编辑截图不可用。"),
        );
        await workspace.dispatch({ type: "capture.cancel" });
        closeWindow();
        return;
      }
      setPending(result.data);
      const source = new Image();
      source.onload = () => {
        if (!disposed) setImage(source);
      };
      source.onerror = async () => {
        if (disposed) return;
        setError(english ? "Unable to load the capture." : "无法加载截图。");
        await workspace.dispatch({ type: "capture.cancel" });
        closeWindow();
      };
      source.src = result.data.data_url;
    });
    return () => {
      disposed = true;
    };
  }, [english, workspace]);

  useEffect(() => {
    const update = () =>
      setViewport({ width: globalThis.innerWidth, height: globalThis.innerHeight });
    addEventListener("resize", update);
    return () => removeEventListener("resize", update);
  }, []);

  useEffect(() => {
    if (!selectionReady || !toolbar.current) return undefined;
    const update = () => {
      const bounds = toolbar.current?.getBoundingClientRect();
      if (bounds?.width && bounds?.height) {
        setToolbarSize({ width: bounds.width, height: bounds.height });
      }
    };
    update();
    const observer = globalThis.ResizeObserver
      ? new ResizeObserver(update)
      : null;
    observer?.observe(toolbar.current);
    return () => observer?.disconnect();
  }, [selectionReady]);

  useEffect(() => {
    if (!image || !selection || !canvas.current) return;
    const target = canvas.current;
    const scaleX = image.naturalWidth / viewport.width;
    const scaleY = image.naturalHeight / viewport.height;
    target.width = Math.max(1, Math.round(selection.width * scaleX));
    target.height = Math.max(1, Math.round(selection.height * scaleY));
    const context = target.getContext("2d");
    context.drawImage(
      image,
      Math.round(selection.x * scaleX),
      Math.round(selection.y * scaleY),
      target.width,
      target.height,
      0,
      0,
      target.width,
      target.height,
    );
    mosaic.current = mosaicForCanvas(target);
    for (const operation of history.operations) {
      drawCaptureOperation(context, operation, mosaic.current);
    }
    if (draft) drawCaptureOperation(context, draft, mosaic.current);
  }, [draft, history.operations, image, selection, viewport]);

  useEffect(() => {
    const shortcut = (event) => {
      const editing = event.target.closest?.("input, textarea, [contenteditable='true']");
      if (editing) return;
      if (event.key === "Escape") {
        event.preventDefault();
        cancelRef.current?.();
        return;
      }
      if (!(event.metaKey || event.ctrlKey) || event.key.toLowerCase() !== "z") {
        return;
      }
      event.preventDefault();
      changeHistory((current) =>
        event.shiftKey
          ? redoCaptureOperation(current)
          : undoCaptureOperation(current),
      );
    };
    addEventListener("keydown", shortcut);
    return () => removeEventListener("keydown", shortcut);
  }, []);

  const capturePointer = (event) => {
    try {
      stage.current?.setPointerCapture(event.pointerId);
    } catch {
      // The pointer may already have been released.
    }
  };

  const beginSelection = (event) => {
    if (!image || busy || event.button !== 0) return;
    if (event.target.closest(".capture-toolbar")) return;
    if (selectionLocked) return;
    const point = pointInViewport(event, stage.current);
    interactionRef.current = { kind: "create", start: point };
    setSelectionReady(false);
    setTool(null);
    setStyleOpen(false);
    changeSelection({ x: point.x, y: point.y, width: 0, height: 0 });
    capturePointer(event);
    event.preventDefault();
  };

  const beginResize = (event, handle) => {
    if (selectionLocked || busy || event.button !== 0) return;
    event.stopPropagation();
    interactionRef.current = {
      kind: "resize",
      handle,
      selection: selectionRef.current,
    };
    capturePointer(event);
  };

  const beginInsideSelection = (event) => {
    if (busy || event.button !== 0 || !canvas.current) return;
    event.stopPropagation();
    if (tool === "text") return;
    const point = pointOnCanvas(event, canvas.current);
    if (tool) {
      const scale = canvas.current.width / selectionRef.current.width;
      interactionRef.current = { kind: "draw" };
      changeDraft(
        tool === "pen" || tool === "mosaic"
          ? { tool, color, size: size * scale, points: [point] }
          : {
              tool,
              color,
              size: size * scale,
              start: point,
              end: point,
            },
      );
    } else if (!selectionLocked) {
      interactionRef.current = {
        kind: "move",
        start: pointInViewport(event, stage.current),
        selection: selectionRef.current,
      };
    }
    capturePointer(event);
    event.preventDefault();
  };

  const placeText = (event) => {
    if (busy || tool !== "text" || !canvas.current) return;
    event.stopPropagation();
    const point = pointOnCanvas(event, canvas.current);
    const scale = canvas.current.width / selectionRef.current.width;
    changeTextInput({
      ...point,
      value: "",
      color,
      size: size * scale,
      displaySize: size,
    });
  };

  const movePointer = (event) => {
    const interaction = interactionRef.current;
    if (!interaction) return;
    const point = pointInViewport(event, stage.current);
    if (interaction.kind === "create") {
      changeSelection(
        normalizeCaptureSelection(interaction.start, point, viewport),
      );
    } else if (interaction.kind === "move") {
      changeSelection(
        moveCaptureSelection(
          interaction.selection,
          {
            x: point.x - interaction.start.x,
            y: point.y - interaction.start.y,
          },
          viewport,
        ),
      );
    } else if (interaction.kind === "resize") {
      changeSelection(
        resizeCaptureSelection(
          interaction.selection,
          interaction.handle,
          point,
          viewport,
        ),
      );
    } else if (interaction.kind === "draw" && draftRef.current) {
      const canvasPoint = pointOnCanvas(event, canvas.current);
      const current = draftRef.current;
      changeDraft(
        current.points
          ? { ...current, points: [...current.points, canvasPoint] }
          : { ...current, end: canvasPoint },
      );
    }
  };

  const endPointer = (event) => {
    const interaction = interactionRef.current;
    interactionRef.current = null;
    if (!interaction) return;
    if (interaction.kind === "draw") {
      const current = draftRef.current;
      const useful = current?.points
        ? current.points.length > 1
        : current &&
          Math.hypot(
            current.end.x - current.start.x,
            current.end.y - current.start.y,
          ) > 2;
      if (useful) addOperation(current);
      changeDraft(null);
    } else if (interaction.kind === "create") {
      const current = selectionRef.current;
      if (!current || current.width < 24 || current.height < 24) {
        changeSelection(null);
        setSelectionReady(false);
      } else {
        setSelectionReady(true);
      }
    } else {
      setSelectionReady(true);
    }
    try {
      stage.current?.releasePointerCapture(event.pointerId);
    } catch {
      // Pointer cancellation already releases capture.
    }
  };

  const placement =
    selectionReady && selection
      ? placeCaptureToolbar(selection, toolbarSize, viewport)
      : null;
  const scaleX = image ? image.naturalWidth / viewport.width : 1;
  const scaleY = image ? image.naturalHeight / viewport.height : 1;
  const canExport = Boolean(image && selectionReady && selection);

  return (
    <main
      className="capture-editor"
      onContextMenu={(event) => event.preventDefault()}
    >
      {image && (
        <img
          className="capture-screen"
          src={pending?.data_url}
          alt=""
          draggable="false"
        />
      )}
      <section
        ref={stage}
        className="capture-stage"
        onPointerDown={beginSelection}
        onPointerMove={movePointer}
        onPointerUp={endPointer}
        onPointerCancel={endPointer}
      >
        {!image && !error && (
          <div className="capture-loading">
            {english ? "Loading capture…" : "正在加载截图…"}
          </div>
        )}
        {image && !selection && (
          <div className="capture-hint">
            {english
              ? "Drag to select an area · Esc to cancel"
              : "拖动鼠标选择截图区域 · Esc 取消"}
          </div>
        )}
        {selection && (
          <div
            className={`capture-selection${selectionLocked ? " locked" : ""}${tool ? " drawing" : ""}`}
            style={{
              left: selection.x,
              top: selection.y,
              width: selection.width,
              height: selection.height,
            }}
            onPointerDown={beginInsideSelection}
            onClick={placeText}
          >
            <canvas ref={canvas} />
            {selectionReady && (
              <span
                className="capture-dimensions"
                style={{ top: selection.y < 34 ? 6 : -30 }}
              >
                {Math.round(selection.width * scaleX)} ×{" "}
                {Math.round(selection.height * scaleY)}
              </span>
            )}
            {selectionReady &&
              !selectionLocked &&
              HANDLES.map((handle) => (
                <span
                  className={`capture-handle ${handle}`}
                  key={handle}
                  onPointerDown={(event) => beginResize(event, handle)}
                  aria-hidden="true"
                />
              ))}
            {textInput && (
              <input
                ref={textInputElement}
                className="capture-text-input"
                style={{
                  left: textInput.left,
                  top: textInput.top,
                  width: textInput.width,
                  color: textInput.color,
                  fontSize: `${Math.max(16, textInput.displaySize * 5)}px`,
                }}
                value={textInput.value}
                autoFocus
                placeholder={english ? "Type text" : "输入文字"}
                onPointerDown={(event) => event.stopPropagation()}
                onClick={(event) => event.stopPropagation()}
                onChange={(event) =>
                  changeTextInput({
                    ...textInputRef.current,
                    value: event.target.value,
                  })
                }
                onBlur={commitText}
                onKeyDown={(event) => {
                  event.stopPropagation();
                  if (event.nativeEvent.isComposing) return;
                  if (event.key === "Escape") changeTextInput(null);
                  if (event.key === "Enter") {
                    event.preventDefault();
                    commitText();
                  }
                }}
              />
            )}
          </div>
        )}
        {placement && (
          <div
            ref={toolbar}
            className={`capture-toolbar side-${placement.side}`}
            style={{ left: placement.left, top: placement.top }}
            onPointerDown={(event) => event.stopPropagation()}
            role="toolbar"
            aria-label={english ? "Capture tools" : "截图工具"}
          >
            {TOOLS.map((item) => {
              const label = english ? item.en : item.zh;
              return (
                <button
                  type="button"
                  className={tool === item.value ? "active" : ""}
                  onClick={() => {
                    setTool(item.value);
                    setStyleOpen(tool !== item.value || !styleOpen);
                  }}
                  title={label}
                  aria-label={label}
                  aria-pressed={tool === item.value}
                  key={item.value}
                >
                  <CaptureIcon name={item.value} />
                </button>
              );
            })}
            <span className="capture-toolbar-separator" aria-hidden="true" />
            <button
              type="button"
              onClick={() =>
                changeHistory((current) => undoCaptureOperation(current))
              }
              disabled={!history.operations.length}
              title={english ? "Undo" : "撤销"}
              aria-label={english ? "Undo" : "撤销"}
            >
              <CaptureIcon name="undo" />
            </button>
            <button
              type="button"
              onClick={() =>
                changeHistory((current) => redoCaptureOperation(current))
              }
              disabled={!history.redo.length}
              title={english ? "Redo" : "重做"}
              aria-label={english ? "Redo" : "重做"}
            >
              <CaptureIcon name="redo" />
            </button>
            <span className="capture-toolbar-separator" aria-hidden="true" />
            <button
              type="button"
              onClick={cancel}
              disabled={busy}
              title={english ? "Cancel" : "取消"}
              aria-label={english ? "Cancel" : "取消"}
            >
              <CaptureIcon name="cancel" />
            </button>
            <button
              type="button"
              onClick={pin}
              disabled={!canExport || busy}
              title={english ? "Pin" : "钉图"}
              aria-label={english ? "Pin" : "钉图"}
            >
              <CaptureIcon name="pin" />
            </button>
            <button
              type="button"
              onClick={save}
              disabled={!canExport || busy}
              title={english ? "Save" : "保存"}
              aria-label={english ? "Save" : "保存"}
            >
              <CaptureIcon name="save" />
            </button>
            <button
              type="button"
              className="primary"
              onClick={finish}
              disabled={!canExport || busy}
              title={english ? "Done" : "完成"}
              aria-label={english ? "Done" : "完成"}
            >
              <CaptureIcon name="done" />
            </button>
            {tool && styleOpen && (
              <div className="capture-style-panel" role="group">
                <div
                  className="capture-color-options"
                  aria-label={english ? "Color" : "颜色"}
                >
                  {COLORS.map((value) => (
                    <button
                      type="button"
                      className={color === value ? "selected" : ""}
                      style={{ "--capture-color": value }}
                      onClick={() => setColor(value)}
                      aria-label={`${english ? "Color" : "颜色"} ${value}`}
                      key={value}
                    />
                  ))}
                </div>
                <span className="capture-toolbar-separator" aria-hidden="true" />
                <div
                  className="capture-size-options"
                  aria-label={english ? "Thickness" : "粗细"}
                >
                  {SIZES.map((value) => (
                    <button
                      type="button"
                      className={size === value ? "selected" : ""}
                      onClick={() => setSize(value)}
                      aria-label={`${english ? "Thickness" : "粗细"} ${value}`}
                      key={value}
                    >
                      <span style={{ width: value + 3, height: value + 3 }} />
                    </button>
                  ))}
                </div>
              </div>
            )}
          </div>
        )}
        {(error || status) && (
          <div className={error ? "capture-error" : "capture-status"}>
            {error || status}
          </div>
        )}
      </section>
    </main>
  );
}

export default function CaptureEditor({ workspace, mode = "editor" }) {
  const english = document.documentElement.lang.toLowerCase().startsWith("en");
  return mode === "pin" ? (
    <CapturePin workspace={workspace} english={english} />
  ) : (
    <CaptureOverlay workspace={workspace} english={english} />
  );
}
