import { useEffect, useRef, useState } from "react";
import {
  addCaptureOperation,
  CAPTURE_PIN_TOOLBAR_AREA,
  captureEditorActionAvailability,
  createCaptureHistory,
  createTextOperation,
  drawCaptureOperation,
  expandCaptureWindowSize,
  fitPinnedCapture,
  hitTestCaptureText,
  isCaptureOperationHidden,
  moveCaptureSelection,
  moveCaptureTextOperation,
  nextPinnedCaptureZoom,
  normalizeCaptureSelection,
  placeCaptureTextInput,
  placeCaptureTextControls,
  placePinnedCaptureMenu,
  placePinnedCaptureToolbarBelowImage,
  placeCaptureToolbar,
  redoCaptureOperation,
  replaceCaptureOperation,
  removeCaptureOperation,
  resolveCaptureTextInputFontSize,
  resizeCaptureSelection,
  shouldCancelCaptureTextEdit,
  updateCaptureTextStyle,
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
const TEXT_FONTS = [
  { value: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif', zh: "苹方/微软雅黑", en: "PingFang/Microsoft YaHei" },
  { value: 'Heiti SC, SimHei, sans-serif', zh: "黑体", en: "Heiti" },
  { value: 'Songti SC, SimSun, serif', zh: "宋体", en: "Songti" },
  { value: "Arial, sans-serif", zh: "Arial", en: "Arial" },
  { value: 'ui-monospace, SFMono-Regular, Menlo, monospace', zh: "等宽字体", en: "Monospace" },
];
const TEXT_SIZES = [14, 16, 20, 24, 30, 36];
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
      {name === "delete" && (
        <>
          <path {...common} d="M5 7h14" />
          <path {...common} d="M9 7V4h6v3M7 7l1 13h8l1-13" />
          <path {...common} d="M10 11v5M14 11v5" />
        </>
      )}
      {name === "pin" && (
        <>
          <path {...common} d="m9 4 6 0-1 5 3 3H7l3-3-1-5Z" />
          <path {...common} d="M12 12v8" />
        </>
      )}
      {name === "copy" && (
        <>
          <rect {...common} x="9" y="9" width="11" height="11" rx="1" />
          <path {...common} d="M15 9V4H4v11h5" />
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

function CapturePin({ workspace, english, onEdit, initialShadow = true }) {
  const [pending, setPending] = useState(null);
  const [zoom, setZoom] = useState(1);
  const [menu, setMenu] = useState(null);
  const [shadow, setShadow] = useState(initialShadow);
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
        if (
          event.button !== 0 ||
          event.target.closest("button, [role='menu'], [role='toolbar']")
        ) {
          return;
        }
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
            role="menuitem"
            onClick={() => {
              setMenu(null);
              onEdit?.({ shadow });
            }}
          >
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

function CaptureOverlay({
  workspace,
  english,
  pinEditing = false,
  pinViewport = null,
  onExitPinEdit,
}) {
  const [pending, setPending] = useState(null);
  const [image, setImage] = useState(null);
  const [viewport, setViewport] = useState(() => ({
    width: pinViewport?.width || globalThis.innerWidth,
    height: pinViewport?.height || globalThis.innerHeight,
  }));
  const [windowViewport, setWindowViewport] = useState(() => ({
    width: globalThis.innerWidth,
    height: globalThis.innerHeight,
  }));
  const [selection, setSelection] = useState(null);
  const [selectionReady, setSelectionReady] = useState(false);
  const [selectionLocked, setSelectionLocked] = useState(false);
  const [tool, setTool] = useState(null);
  const [color, setColor] = useState(COLORS[0]);
  const [size, setSize] = useState(4);
  const [fontFamily, setFontFamily] = useState(TEXT_FONTS[0].value);
  const [fontSize, setFontSize] = useState(20);
  const [fontWeight, setFontWeight] = useState("600");
  const [fontStyle, setFontStyle] = useState("normal");
  const [history, setHistory] = useState(createCaptureHistory);
  const [draft, setDraft] = useState(null);
  const [textInput, setTextInput] = useState(null);
  const [styleOpen, setStyleOpen] = useState(false);
  const [toolbarSize, setToolbarSize] = useState({ width: 560, height: 52 });
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

  const updateTextStyle = (style) => {
    if (style.color !== undefined) setColor(style.color);
    if (style.fontFamily !== undefined) setFontFamily(style.fontFamily);
    if (style.fontWeight !== undefined) setFontWeight(style.fontWeight);
    if (style.fontStyle !== undefined) setFontStyle(style.fontStyle);
    if (style.fontSize !== undefined) setFontSize(Number(style.fontSize));
    const current = textInputRef.current;
    if (!current) return;
    const scale = canvas.current && selectionRef.current
      ? canvas.current.width / Math.max(1, selectionRef.current.width)
      : 1;
    changeTextInput(updateCaptureTextStyle(current, style, scale));
    requestAnimationFrame(() => {
      const target = textInputElement.current;
      target?.focus({ preventScroll: true });
    });
  };

  const removeTextInput = () => {
    const current = textInputRef.current;
    if (current?.original?.id) {
      changeHistory((history) => removeCaptureOperation(history, current.original.id));
    }
    changeTextInput(null);
  };

  const addOperation = (operation) => {
    changeHistory((current) => addCaptureOperation(current, operation));
    setSelectionLocked(true);
  };

  const measureCaptureText = (text, height, operation = {}) => {
    const context = canvas.current?.getContext("2d");
    if (!context) return 0;
    context.save();
    context.font = `${operation.fontStyle || "normal"} ${operation.fontWeight || "600"} ${height}px ${operation.fontFamily || TEXT_FONTS[0].value}`;
    const width = context.measureText(text).width;
    context.restore();
    return width;
  };

  const commitText = () => {
    const current = textInputRef.current;
    if (!current || !canvas.current) return null;
    changeTextInput(null);
    if (shouldCancelCaptureTextEdit(current.original, current.value)) {
      return null;
    }
    let operation = createTextOperation(
      current,
      current.color,
      current.size,
      {
        fontFamily: current.fontFamily,
        fontWeight: current.fontWeight,
        fontStyle: current.fontStyle,
        fontSize: current.fontSize,
      },
    );
    if (!operation) return null;
    operation = moveCaptureTextOperation(
      operation,
      { x: 0, y: 0 },
      canvas.current,
      measureCaptureText,
    );
    if (current.original) {
      const unchanged =
        operation.text === current.original.text &&
        operation.start.x === current.original.start.x &&
        operation.start.y === current.original.start.y &&
        operation.fontFamily === (current.original.fontFamily || TEXT_FONTS[0].value) &&
        operation.fontWeight === (current.original.fontWeight || "600") &&
        operation.fontStyle === (current.original.fontStyle || "normal") &&
        operation.fontSize === (current.original.fontSize || current.original.size * 5);
      if (!unchanged) {
        changeHistory((history) =>
          replaceCaptureOperation(history, current.id, operation),
        );
      }
    } else {
      addOperation(operation);
    }
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
    if (pinEditing) {
      changeTextInput(null);
      onExitPinEdit?.();
      return;
    }
    setBusy(true);
    try {
      await workspace.dispatch({ type: "capture.cancel" });
    } finally {
      closeWindow();
    }
  };
  cancelRef.current = cancel;

  const updatePinnedCapture = async (saveAfter = false) => {
    setBusy(true);
    setError("");
    setStatus("");
    const updated = await workspace.dispatch({
      type: "capture.pin",
      dataUrl: exportPng(),
    });
    if (!updated.ok) {
      setBusy(false);
      setError(updated.error.message);
      return false;
    }
    if (saveAfter) {
      const saved = await workspace.dispatch({ type: "capture.pin.save" });
      if (!saved.ok) {
        setBusy(false);
        setError(saved.error.message);
        return false;
      }
    }
    setBusy(false);
    onExitPinEdit?.();
    return true;
  };

  const finish = async () => {
    if (pinEditing) {
      await updatePinnedCapture();
      return;
    }
    if (!pending?.conversation_id) {
      setError(english ? "Select a conversation first." : "请先选择一个会话。");
      return;
    }
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
    if (pinEditing) {
      await updatePinnedCapture();
      return;
    }
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

  const copy = async () => {
    if (pinEditing) {
      setBusy(true);
      setError("");
      setStatus("");
      const updated = await workspace.dispatch({
        type: "capture.pin",
        dataUrl: exportPng(),
      });
      if (!updated.ok) {
        setBusy(false);
        setError(updated.error.message);
        return;
      }
      const copied = await workspace.dispatch({
        type: "capture.pin.copy",
        scale: null,
      });
      setBusy(false);
      if (!copied.ok) {
        setError(copied.error.message);
        return;
      }
      onExitPinEdit?.();
      return;
    }
    setBusy(true);
    setError("");
    setStatus("");
    const result = await workspace.dispatch({
      type: "capture.copy",
      dataUrl: exportPng(),
    });
    setBusy(false);
    if (!result.ok) {
      setError(result.error.message);
      return;
    }
    closeWindow();
  };

  const save = async () => {
    if (pinEditing) {
      await updatePinnedCapture(true);
      return;
    }
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
        if (pinEditing) {
          onExitPinEdit?.();
          return;
        }
        await workspace.dispatch({ type: "capture.cancel" });
        closeWindow();
        return;
      }
      setPending(result.data);
      const source = new Image();
      source.onload = () => {
        if (disposed) return;
        setImage(source);
        if (pinEditing) {
          const captureViewport = pinViewport || {
            width: globalThis.innerWidth,
            height: globalThis.innerHeight,
          };
          const fullSelection = {
            x: 0,
            y: 0,
            width: captureViewport.width,
            height: captureViewport.height,
          };
          changeSelection(fullSelection);
          setSelectionReady(true);
          setSelectionLocked(true);
        }
      };
      source.onerror = async () => {
        if (disposed) return;
        setError(english ? "Unable to load the capture." : "无法加载截图。");
        if (pinEditing) {
          onExitPinEdit?.();
          return;
        }
        await workspace.dispatch({ type: "capture.cancel" });
        closeWindow();
      };
      source.src = result.data.data_url;
    });
    return () => {
      disposed = true;
    };
  }, [english, onExitPinEdit, pinEditing, pinViewport, workspace]);

  useEffect(() => {
    const update = () => {
      const next = { width: globalThis.innerWidth, height: globalThis.innerHeight };
      setWindowViewport(next);
      if (!pinEditing) setViewport(next);
    };
    update();
    addEventListener("resize", update);
    return () => removeEventListener("resize", update);
  }, [pinEditing]);

  useEffect(() => {
    if (pinEditing && pinViewport) setViewport(pinViewport);
  }, [pinEditing, pinViewport]);

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
    const hiddenTextId =
      draft?.tool === "text" ? draft.id : textInput?.original?.id;
    for (const operation of history.operations) {
      if (isCaptureOperationHidden(operation, hiddenTextId)) continue;
      drawCaptureOperation(context, operation, mosaic.current);
    }
    if (draft) drawCaptureOperation(context, draft, mosaic.current);
  }, [
    draft,
    history.operations,
    image,
    selection,
    textInput?.original?.id,
    viewport,
  ]);

  useEffect(() => {
    const shortcut = (event) => {
      const editing = event.target.closest?.("input, textarea, [contenteditable='true']");
      if (editing) return;
      if (event.key === "Escape") {
        event.preventDefault();
        if (interactionRef.current?.kind === "text") {
          interactionRef.current = null;
          changeDraft(null);
          return;
        }
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
    const point = pointOnCanvas(event, canvas.current);
    if (tool === "text") {
      if (textInputRef.current) commitText();
      interactionRef.current = {
        kind: "text",
        operation: hitTestCaptureText(
          historyRef.current.operations,
          point,
          measureCaptureText,
        ),
        start: point,
        startClient: { x: event.clientX, y: event.clientY },
        moved: false,
      };
      capturePointer(event);
      event.preventDefault();
      return;
    }
    if (tool) {
      const scale = canvas.current.width / selectionRef.current.width;
      interactionRef.current = { kind: "draw" };
      changeDraft(
        tool === "pen" || tool === "mosaic"
          ? { tool, color: tool === "mosaic" ? "#000000" : color, size: size * scale, points: [point] }
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

  const openTextInput = (operation, point) => {
    if (!canvas.current || !selectionRef.current) return;
    const scale = canvas.current.width / selectionRef.current.width;
    const source = operation ?? {
      id: `capture-text-${++textInputSequence.current}`,
      start: point,
      text: "",
      color,
      size: size * scale,
      fontFamily,
      fontWeight,
      fontStyle,
      fontSize: fontSize * scale,
    };
    const textMetrics = resolveCaptureTextInputFontSize(source, scale, fontSize);
    if (operation) {
      setColor(source.color || color);
      setFontFamily(source.fontFamily || fontFamily);
      setFontWeight(source.fontWeight || fontWeight);
      setFontStyle(source.fontStyle || fontStyle);
      // The toolbar controls are in display/CSS pixels.  Operations store
      // canvas pixels, so convert back before the value is reused for a new
      // annotation.  Otherwise the next open multiplies an already-scaled
      // value by `scale` and the input grows on every edit cycle.
      setFontSize(textMetrics.displayFontSize);
    }
    const placement = placeCaptureTextInput(
      source.start,
      canvas.current,
      selectionRef.current,
    );
    changeTextInput({
      id: source.id,
      x: source.start.x,
      y: source.start.y,
      value: source.text,
      color: source.color,
      size: source.size,
      displaySize: source.size / scale,
      fontFamily: source.fontFamily || fontFamily,
      fontWeight: source.fontWeight || fontWeight,
      fontStyle: source.fontStyle || fontStyle,
      fontSize: textMetrics.fontSize,
      displayFontSize: textMetrics.displayFontSize,
      original: operation ?? null,
      ...placement,
    });
  };

  const beginTextInputMove = (event) => {
    if (busy || event.button !== 0 || !textInputRef.current) return;
    event.stopPropagation();
    event.preventDefault();
    interactionRef.current = {
      kind: "text-input-move",
      input: textInputRef.current,
      startClient: { x: event.clientX, y: event.clientY },
    };
    capturePointer(event);
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
    } else if (interaction.kind === "text") {
      if (
        !interaction.moved &&
        Math.hypot(
          event.clientX - interaction.startClient.x,
          event.clientY - interaction.startClient.y,
        ) <= 4
      ) {
        return;
      }
      interaction.moved = true;
      if (!interaction.operation) return;
      const canvasPoint = pointOnCanvas(event, canvas.current);
      changeDraft(
        moveCaptureTextOperation(
          interaction.operation,
          {
            x: canvasPoint.x - interaction.start.x,
            y: canvasPoint.y - interaction.start.y,
          },
          canvas.current,
          measureCaptureText,
        ),
      );
    } else if (
      interaction.kind === "text-input-move" &&
      canvas.current &&
      selectionRef.current
    ) {
      const input = interaction.input;
      const selectionBounds = selectionRef.current;
      const padding = 6;
      const editorHeight = textInputElement.current?.offsetHeight || 36;
      const left = Math.min(
        Math.max(
          padding,
          input.left + event.clientX - interaction.startClient.x,
        ),
        Math.max(padding, selectionBounds.width - input.width - padding),
      );
      const top = Math.min(
        Math.max(padding, input.top + event.clientY - interaction.startClient.y),
        Math.max(padding, selectionBounds.height - editorHeight - padding),
      );
      changeTextInput({
        ...input,
        left,
        top,
        x: (left / selectionBounds.width) * canvas.current.width,
        y: (top / selectionBounds.height) * canvas.current.height,
      });
    }
  };

  const endPointer = (event) => {
    const interaction = interactionRef.current;
    interactionRef.current = null;
    if (!interaction) return;
    if (interaction.kind === "text-input-move") {
      if (event.type === "pointercancel") {
        changeTextInput(interaction.input);
      } else {
        commitText();
      }
    } else if (interaction.kind === "text") {
      let moved = draftRef.current;
      if (
        event.type !== "pointercancel" &&
        !interaction.moved &&
        Math.hypot(
          event.clientX - interaction.startClient.x,
          event.clientY - interaction.startClient.y,
        ) > 4
      ) {
        interaction.moved = true;
        if (interaction.operation) {
          const canvasPoint = pointOnCanvas(event, canvas.current);
          moved = moveCaptureTextOperation(
            interaction.operation,
            {
              x: canvasPoint.x - interaction.start.x,
              y: canvasPoint.y - interaction.start.y,
            },
            canvas.current,
            measureCaptureText,
          );
        }
      }
      changeDraft(null);
      if (event.type !== "pointercancel") {
        if (interaction.moved) {
          if (
            moved &&
            interaction.operation &&
            (moved.start.x !== interaction.operation.start.x ||
              moved.start.y !== interaction.operation.start.y)
          ) {
            changeHistory((current) =>
              replaceCaptureOperation(
                current,
                interaction.operation.id,
                moved,
              ),
            );
          }
        } else if (interaction.operation) {
          openTextInput(interaction.operation);
        } else {
          openTextInput(null, interaction.start);
        }
      }
    } else if (interaction.kind === "draw") {
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
      ? pinEditing
        ? (() => {
            const below = placePinnedCaptureToolbarBelowImage(
              { x: 0, y: 0, width: viewport.width, height: viewport.height },
              toolbarSize,
              windowViewport,
            );
            return { left: below.left, top: below.top, side: "bottom" };
          })()
        : placeCaptureToolbar(selection, toolbarSize, viewport)
      : null;
  const textControls = textInput && selection
    ? placeCaptureTextControls(textInput, selection.width)
    : null;
  const scaleX = image ? image.naturalWidth / viewport.width : 1;
  const scaleY = image ? image.naturalHeight / viewport.height : 1;
  const canExport = Boolean(image && selectionReady && selection);
  const actionAvailability = captureEditorActionAvailability({
    conversationId: pending?.conversation_id,
    nativeCopy: Boolean(globalThis.__TAURI__?.core?.invoke),
    pinEditing,
  });
  const finishLabel = actionAvailability.canFinish
    ? english
      ? "Done"
      : "完成"
    : english
      ? "Select a conversation first"
      : "请先选择一个会话";

  return (
    <main
      className={`capture-editor${pinEditing ? " capture-pin-editing" : ""}`}
      style={pinEditing ? { "--capture-pin-image-height": `${viewport.height}px` } : undefined}
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
            className={`capture-selection${selectionLocked ? " locked" : ""}${tool ? ` drawing tool-${tool}` : ""}`}
            style={{
              left: selection.x,
              top: selection.y,
              width: selection.width,
              height: selection.height,
            }}
            onPointerDown={beginInsideSelection}
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
              <>
                <input
                  ref={textInputElement}
                  className="capture-text-input"
                  style={{
                    left: textInput.left,
                    top: textInput.top,
                    width: textInput.width,
                    color: textInput.color,
                    fontFamily: textInput.fontFamily,
                    fontWeight: textInput.fontWeight,
                    fontStyle: textInput.fontStyle,
                    fontSize: `${textInput.displayFontSize || Math.max(16, textInput.displaySize * 5)}px`,
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
                  onBlur={(event) => {
                    if (event.relatedTarget?.closest?.(".capture-style-panel, .capture-text-delete-button")) return;
                    commitText();
                  }}
                  onKeyDown={(event) => {
                    event.stopPropagation();
                    if (event.nativeEvent.isComposing) return;
                    if (event.key === "Escape") {
                      event.preventDefault();
                      changeTextInput(null);
                    }
                    if (event.key === "Enter") {
                      event.preventDefault();
                      commitText();
                    }
                  }}
                />
                {textControls && (
                  <div
                    className="capture-text-controls"
                    style={{
                      left: textControls.left,
                      top: textControls.top,
                      width: textControls.width,
                      height: textControls.height,
                    }}
                  >
                    <button
                      type="button"
                      className="capture-text-delete-button"
                      title={english ? "Delete text" : "删除文字"}
                      aria-label={english ? "Delete text" : "删除文字"}
                      onPointerDown={(event) => {
                        event.preventDefault();
                        event.stopPropagation();
                      }}
                      onClick={removeTextInput}
                    >
                      <CaptureIcon name="delete" />
                    </button>
                    <button
                      type="button"
                      className="capture-text-drag-handle"
                      tabIndex={-1}
                      title={english ? "Move text" : "移动文字"}
                      aria-label={english ? "Move text" : "移动文字"}
                      onPointerDown={beginTextInputMove}
                    >
                      <span aria-hidden="true">⠿</span>
                    </button>
                  </div>
                )}
              </>
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
              disabled={!history.undo.length}
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
            {actionAvailability.canCopy && (
              <button
                type="button"
                onClick={copy}
                disabled={!canExport || busy}
                title={english ? "Copy" : "复制"}
                aria-label={english ? "Copy" : "复制"}
              >
                <CaptureIcon name="copy" />
              </button>
            )}
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
              disabled={!canExport || busy || !actionAvailability.canFinish}
              title={finishLabel}
              aria-label={finishLabel}
            >
              <CaptureIcon name="done" />
            </button>
            {tool && styleOpen && (
              <div
                className="capture-style-panel"
                role="group"
                onPointerDown={(event) => {
                  if (event.target.closest("button")) event.preventDefault();
                }}
              >
                {tool !== "mosaic" && tool !== "text" && <div
                  className="capture-color-options"
                  aria-label={english ? "Color" : "颜色"}
                >
                  {COLORS.map((value) => (
                    <button
                      type="button"
                      className={color === value ? "selected" : ""}
                      style={{ "--capture-color": value }}
                      onClick={() => updateTextStyle({ color: value })}
                      aria-label={`${english ? "Color" : "颜色"} ${value}`}
                      key={value}
                    />
                  ))}
                </div>}
                {tool !== "text" && <span className="capture-toolbar-separator" aria-hidden="true" />}
                {tool !== "text" && <div
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
                </div>}
                {tool === "text" && (
                  <>
                    <span className="capture-toolbar-separator" aria-hidden="true" />
                    <select
                      aria-label={english ? "Font family" : "字体"}
                      value={fontFamily}
                      onChange={(event) => updateTextStyle({ fontFamily: event.target.value })}
                    >
                      {TEXT_FONTS.map((item) => <option value={item.value} key={item.value}>{english ? item.en : item.zh}</option>)}
                    </select>
                    <select
                      aria-label={english ? "Font size" : "字号"}
                      value={fontSize}
                      onChange={(event) => updateTextStyle({ fontSize: Number(event.target.value) })}
                    >
                      {TEXT_SIZES.map((value) => <option value={value} key={value}>{value}px</option>)}
                    </select>
                    <div className="capture-color-options" aria-label={english ? "Color" : "颜色"}>
                      {COLORS.map((value) => <button type="button" className={color === value ? "selected" : ""} style={{ "--capture-color": value }} onClick={() => updateTextStyle({ color: value })} aria-label={`${english ? "Color" : "颜色"} ${value}`} key={value} />)}
                    </div>
                    <button type="button" className={fontWeight === "700" ? "selected" : ""} onClick={() => updateTextStyle({ fontWeight: fontWeight === "700" ? "600" : "700" })} aria-pressed={fontWeight === "700"}>B</button>
                    <button type="button" className={fontStyle === "italic" ? "selected" : ""} onClick={() => updateTextStyle({ fontStyle: fontStyle === "italic" ? "normal" : "italic" })} aria-pressed={fontStyle === "italic"}><i>I</i></button>
                  </>
                )}
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
  const [pinEditing, setPinEditing] = useState(false);
  const [pinImageViewport, setPinImageViewport] = useState(null);
  const [pinShadowBeforeEdit, setPinShadowBeforeEdit] = useState(true);
  const pinWindowGeometry = useRef(null);
  useEffect(() => {
    if (mode !== "pin") return undefined;
    const elements = [
      document.documentElement,
      document.body,
      document.getElementById("root"),
    ].filter(Boolean);
    elements.forEach((element) => element.classList.add("capture-view-transparent"));
    return () => elements.forEach((element) => element.classList.remove("capture-view-transparent"));
  }, [mode]);
  useEffect(() => {
    if (mode !== "pin") return undefined;
    const windowApi = globalThis.__TAURI__?.window?.getCurrentWindow?.();
    if (!windowApi) return undefined;
    let disposed = false;
    if (pinEditing) {
      // The normal capture editor has no native window shadow.  Disable the
      // pin window shadow while the transparent toolbar strip is visible so
      // the strip does not acquire a gray outline around the whole window.
      void windowApi.setShadow?.(false);
      Promise.all([
        windowApi.innerSize?.(),
        windowApi.innerPosition?.(),
        windowApi.scaleFactor?.(),
      ]).then(([size, position, scaleFactor]) => {
        if (disposed || !size) return;
        const factor = Number(scaleFactor) > 0
          ? Number(scaleFactor)
          : Number(globalThis.devicePixelRatio) > 0
            ? Number(globalThis.devicePixelRatio)
            : 1;
        const originalSize = {
          type: "Physical",
          width: Number(size.width),
          height: Number(size.height),
        };
        const expandedSize = expandCaptureWindowSize(
          originalSize,
          factor,
          CAPTURE_PIN_TOOLBAR_AREA,
        );
        if (!expandedSize) return;
        const originalPosition = position
          ? {
              type: "Physical",
              x: Number(position.x),
              y: Number(position.y),
            }
          : null;
        pinWindowGeometry.current = {
          size: originalSize,
          position: originalPosition,
        };
        return windowApi.setSize?.(expandedSize);
      }).catch(() => {});
    } else {
      const geometry = pinWindowGeometry.current;
      pinWindowGeometry.current = null;
      if (geometry) {
        Promise.resolve(windowApi.setSize?.(geometry.size)).catch(() => {});
        if (geometry.position) Promise.resolve(windowApi.setPosition?.(geometry.position)).catch(() => {});
      }
      Promise.resolve(windowApi.setShadow?.(pinShadowBeforeEdit)).catch(() => {});
    }
    return () => { disposed = true; };
  }, [mode, pinEditing, pinShadowBeforeEdit]);
  const enterPinEdit = ({ shadow = true } = {}) => {
    setPinShadowBeforeEdit(Boolean(shadow));
    setPinImageViewport({
      width: globalThis.innerWidth,
      height: globalThis.innerHeight,
    });
    setPinEditing(true);
  };
  const exitPinEdit = () => {
    setPinEditing(false);
    setPinImageViewport(null);
  };
  return mode === "pin" && !pinEditing ? (
    <CapturePin
      workspace={workspace}
      english={english}
      onEdit={enterPinEdit}
      initialShadow={pinShadowBeforeEdit}
    />
  ) : (
    <CaptureOverlay
      workspace={workspace}
      english={english}
      pinEditing={mode === "pin"}
      pinViewport={pinImageViewport}
      onExitPinEdit={
        mode === "pin" ? exitPinEdit : undefined
      }
    />
  );
}
