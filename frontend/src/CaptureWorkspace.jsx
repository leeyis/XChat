import { useEffect, useRef, useState } from "react";
import CaptureSurface, { CaptureIcon } from "./CaptureSurface.jsx";
import CapturePin, { captureDispatch } from "./CapturePin.jsx";
import { captureRecord, captureRecords, captureSettings, patchCaptureRecord, saveCaptureSettings, watchCaptureLibrary } from "./capture-library.js";
import { CAPTURE_FONTS, captureId, normalizePinView } from "./capture-model.js";
import { captureCanvas, captureImage, captureTextLayout, exportCapture } from "./capture-renderer.js";
import { colorSource } from "./capture-sources.js";
import "./capture-widgets.css";

const nativeCapture = () => Boolean(globalThis.__TAURI__?.core?.invoke || globalThis.__TAURI_INTERNALS__?.invoke);
function Button({ children, icon, primary = false, small = false, ...props }) {
  return <button type="button" className={`cap-button${primary ? " primary" : ""}${small ? " small" : ""}`} {...props}>{icon && <CaptureIcon name={icon} />}{children}</button>;
}
function textSource(text) {
  if (!text.trim()) throw new Error("请输入文字内容");
  const probe = captureCanvas(1, 1), context = probe.getContext("2d");
  const operation = { id: captureId(), tool: "text", text, start: { x: 24, y: 24 }, size: 20, fontSize: 20, fontFamily: CAPTURE_FONTS[0][0], color: "#4b624e" };
  const layout = captureTextLayout(operation, context);
  if (layout.width > 12000 || layout.height > 12000) throw new Error("文字内容过长，请分成几张贴图");
  const canvas = captureCanvas(Math.max(320, layout.width + 48), Math.max(140, layout.height + 48));
  canvas.getContext("2d").fillStyle = "#fffef5";
  canvas.getContext("2d").fillRect(0, 0, canvas.width, canvas.height);
  const region = { x: 0, y: 0, width: canvas.width, height: canvas.height };
  const document = { version: 1, base: canvas.toDataURL("image/png"), region, operations: [operation] };
  return { data_url: exportCapture(canvas, [operation], region).toDataURL("image/png"), width: canvas.width, height: canvas.height, title: "文字便签", document };
}
async function imageSource(dataUrl, title) {
  const image = await captureImage(dataUrl);
  if (!image.naturalWidth || image.naturalWidth * image.naturalHeight > 64000000) throw new Error("图片过大，请选择 6400 万像素以内的图片");
  const canvas = captureCanvas(image.naturalWidth, image.naturalHeight);
  canvas.getContext("2d").drawImage(image, 0, 0);
  return { data_url: canvas.toDataURL("image/png"), width: canvas.width, height: canvas.height, title };
}

export default function CaptureWorkspace({ workspace, english = false, captureShortcut = "" }) {
  const t = (zh, en) => english ? en : zh;
  const native = nativeCapture();
  const [open, setOpen] = useState(false), [panel, setPanel] = useState("pins"), [records, setRecords] = useState([]), [settings, setSettings] = useState(captureSettings), [pending, setPending] = useState(null), [source, setSource] = useState(null), [sourceError, setSourceError] = useState(""), [busy, setBusy] = useState(false), [toast, setToast] = useState(null);
  const [deferredPins, setDeferredPins] = useState(() => new Set());
  const [countdown, setCountdown] = useState(0);
  const [cursorSupported, setCursorSupported] = useState(null);
  const live = useRef(null), restored = useRef(false), sourceInput = useRef(null), lastFocus = useRef(null);
  const countdownSession = useRef(null);
  live.current = { open, pending, settings, records, source, countdown };
  const pins = records.filter(item => item.kind === "pin"), history = records.filter(item => item.kind === "history"), group = settings.group || "默认";
  const groupPins = pins.filter(item => normalizePinView(item.view).group === group);

  function notify(message, error = false, restoreId = null) { setToast({ message, error, restoreId }); }
  function revealPin(id) { setDeferredPins(current => { const next = new Set(current); next.delete(id); return next; }); }
  async function run(action, success) {
    try { const result = await captureDispatch(workspace, action); if (action.type === "capture.pin.restore") revealPin(action.pinId); if (success && result !== null) notify(success); return result; }
    catch (cause) { notify(cause.message, true); return null; }
  }
  async function load() {
    const items = await captureRecords();
    setRecords(items); setSettings(captureSettings()); return items;
  }
  useEffect(() => {
    let disposed = false;
    const viewWrites = new Map();
    const stops = [];
    const refresh = async () => { try { if (!disposed) await load(); } catch (cause) { if (!disposed) notify(cause.message, true); } };
    void (async () => {
      try {
        const preferences = await captureDispatch(workspace, { type: "capture.preferences", delaySeconds: captureSettings().delay });
        if (disposed) return;
        setCursorSupported(preferences?.cursorSupported === true);
        if (native && preferences?.cursorSupported) await captureDispatch(workspace, { type: "capture.preferences", captureCursor: captureSettings().captureCursor });
        const items = await captureRecords();
        if (disposed) return;
        setRecords(items);
        if (!captureSettings().remember) setDeferredPins(new Set(items.filter(item => item.kind === "pin").map(item => item.id)));
        if (native && captureSettings().remember && !restored.current) {
          restored.current = true;
          await captureDispatch(workspace, { type: "capture.group", group: captureSettings().group || "默认" });
          for (const item of items.filter(value => value.kind === "pin" && !value.view?.hidden)) {
            if (disposed) break;
            await captureDispatch(workspace, { type: "capture.pin.restore", pinId: item.id });
          }
        }
      } catch (cause) { if (!disposed) notify(cause.message, true); }
    })();
    stops.push(watchCaptureLibrary(refresh));
    const openCapture = event => {
      if (!event.detail?.data_url) return;
      setPending(event.detail); setSource(null);
    };
    const openWorkspace = event => {
      setOpen(true); setPanel(["pins", "history", "settings"].includes(event.detail?.panel) ? event.detail.panel : "pins");
      if (event.detail?.panel === "sources") setSource({ kind: "text", text: "", color: "#18AC71", file: null });
    };
    const onCountdown = event => {
      const value = event.detail || event.payload || {};
      const remaining = Math.max(0, Number(value.remaining) || 0);
      if (!remaining && value.session_id && countdownSession.current && countdownSession.current !== value.session_id) return;
      countdownSession.current = remaining ? value.session_id || null : null;
      setCountdown(remaining);
    };
    const keyboard = event => {
      if (event.key === "Escape" && live.current.countdown > 0) { event.preventDefault(); event.stopPropagation(); void run({ type: "capture.cancel-start", sessionId: countdownSession.current }); return; }
      if (event.isComposing || event.target.matches("input,textarea,select,[contenteditable=true]")) return;
      if (event.target.closest("[data-capture-pin],.cap-pin-menu")) return;
      if (event.key === "F3" && !native && !live.current.pending) { event.preventDefault(); setOpen(true); setPanel("pins"); }
      if (event.key === "Escape" && live.current.open && !live.current.pending && !event.defaultPrevented) { event.preventDefault(); if (live.current.source) setSource(null); else setOpen(false); }
    };
    addEventListener("xchat-capture-open", openCapture); addEventListener("xchat-capture-workspace", openWorkspace); addEventListener("xchat-capture-countdown", onCountdown); addEventListener("keydown", keyboard);
    stops.push(() => { removeEventListener("xchat-capture-open", openCapture); removeEventListener("xchat-capture-workspace", openWorkspace); removeEventListener("xchat-capture-countdown", onCountdown); removeEventListener("keydown", keyboard); });
    const events = globalThis.__TAURI__?.event;
    if (events?.listen) {
      events.listen("capture-workspace", event => openWorkspace({ detail: event.payload || { panel: "pins" } })).then(stop => disposed ? stop() : stops.push(stop)).catch(cause => notify(cause.message, true));
      events.listen("capture-countdown", onCountdown).then(stop => disposed ? stop() : stops.push(stop)).catch(cause => notify(cause.message, true));
      events.listen("capture-pin-view-updated", event => {
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
      }).then(stop => disposed ? stop() : stops.push(stop)).catch(cause => notify(cause.message, true));
    }
    return () => {
      disposed = true; stops.forEach(stop => stop());
      viewWrites.forEach(({ timer, view }, id) => { clearTimeout(timer); void patchCaptureRecord(id, { view }).catch(() => {}); });
    };
  }, [workspace, native]);
  useEffect(() => { if (!toast) return undefined; const timer = setTimeout(() => setToast(null), toast.restoreId ? 8000 : 5000); return () => clearTimeout(timer); }, [toast]);
  useEffect(() => {
    if (!source) return undefined;
    lastFocus.current = document.activeElement;
    sourceInput.current?.focus();
    return () => { if (lastFocus.current?.isConnected) lastFocus.current.focus({ preventScroll: true }); };
  }, [source?.kind]);

  async function createPin(item) {
    const pinId = captureId();
    await captureDispatch(workspace, { type: "capture.pin", pinId, dataUrl: item.data_url, document: item.document, width: item.width, height: item.height, conversationId: item.conversation_id ?? null });
    await patchCaptureRecord(pinId, { title: item.title || t("截图", "Capture") });
    await load();
    if (!native) requestAnimationFrame(() => document.querySelector(`[data-capture-pin="${CSS.escape(pinId)}"]`)?.focus({ preventScroll: true }));
    notify(t(native ? "已创建贴图" : "已贴到当前页面", native ? "Pinned image created" : "Pinned to this page"));
    return pinId;
  }
  async function paste() {
    setBusy(true);
    try {
      const value = await captureDispatch(workspace, { type: "capture.clipboard" });
      const item = value?.data_url ? await imageSource(value.data_url, t("剪贴板图片", "Clipboard image")) : value?.text?.trim() ? colorSource(value.text, t("色卡 ", "Color ")) || textSource(value.text) : null;
      if (!item) throw new Error(t("剪贴板中没有可用的图片或文字", "The clipboard contains no image or text"));
      await createPin(item);
    } catch (cause) { notify(cause.message, true); }
    finally { setBusy(false); }
  }
  async function createSource() {
    setBusy(true); setSourceError("");
    try {
      let item;
      if (source.kind === "text") item = textSource(source.text);
      else if (source.kind === "color") {
        item = colorSource(source.color, t("色卡 ", "Color "));
        if (!item) throw new Error(t("请输入 HEX 或 RGB 颜色，例如 #18AC71 或 rgb(24,172,113)", "Enter a HEX or RGB color, such as #18AC71 or rgb(24,172,113)"));
      } else {
        if (!source.file) throw new Error(t("请选择一张图片", "Choose an image"));
        if (source.file.size > 64 * 1024 * 1024) throw new Error(t("请选择 64 MB 以内的图片", "Choose an image smaller than 64 MB"));
        const url = URL.createObjectURL(source.file);
        try { item = await imageSource(url, source.file.name); } finally { URL.revokeObjectURL(url); }
      }
      await createPin(item); setSource(null);
    } catch (cause) { setSourceError(cause.message || t("无法读取该图片", "Cannot read this image")); }
    finally { setBusy(false); }
  }
  function edit(item) {
    setPending({ session_id: item.id || captureId(), pin_id: item.kind === "pin" ? item.id : undefined, data_url: item.data_url, width: item.width, height: item.height, document: item.document, conversation_id: item.kind === "pin" ? item.conversation_id ?? null : null });
  }
  async function whiteboard() {
    const canvas = captureCanvas(1440, 900), ctx = canvas.getContext("2d");
    ctx.fillStyle = "#fff"; ctx.fillRect(0, 0, canvas.width, canvas.height);
    const data = canvas.toDataURL("image/png");
    edit({ id: captureId(), data_url: data, width: canvas.width, height: canvas.height, document: { version: 1, base: data, region: { x: 0, y: 0, width: canvas.width, height: canvas.height }, operations: [] } });
  }
  async function setSetting(key, value) {
    try {
      if (key === "preset") await captureDispatch(workspace, { type: "settings.patch", patch: { capture_shortcut: value === "classic" ? "F1" : "Ctrl+Shift+A" } });
      if (key === "group") await captureDispatch(workspace, { type: "capture.group", group: value });
      if (key === "delay" && native) await captureDispatch(workspace, { type: "capture.preferences", delaySeconds: value });
      if (key === "captureCursor") await captureDispatch(workspace, { type: "capture.preferences", captureCursor: value });
      setSettings(saveCaptureSettings({ [key]: value }));
    } catch (cause) { notify(cause.message, true); }
  }
  async function groupAction(action) {
    setBusy(true);
    try {
      for (const [index, item] of groupPins.entries()) {
        if (action === "hide") await captureDispatch(workspace, { type: "capture.pin.close", pinId: item.id, destroy: false });
        else {
          if (action === "recover" || action === "through") await captureDispatch(workspace, { type: "capture.pin.update", pinId: item.id, view: { ...normalizePinView(item.view), ...(action === "recover" ? { x: 40 + index * 26, y: 60 + index * 24 } : {}), through: false } });
          await captureDispatch(workspace, { type: "capture.pin.restore", pinId: item.id });
          revealPin(item.id);
        }
      }
    } catch (cause) { notify(cause.message, true); }
    finally { setBusy(false); }
  }
  async function locate(item) {
    await run({ type: "capture.pin.update", pinId: item.id, view: { ...normalizePinView(item.view), x: 60, y: 80, through: false } });
    await run({ type: "capture.pin.restore", pinId: item.id });
    if (!native) setOpen(false);
  }
  const settingToggle = (key, title, titleEn, note, noteEn, disabled = false) => <label className="cap-setting-row"><span><strong>{t(title, titleEn)}</strong><small>{t(note, noteEn)}</small></span><input type="checkbox" disabled={disabled} checked={!disabled && Boolean(settings[key])} onChange={event => void setSetting(key, event.target.checked)} /></label>;
  const isHidden = item => item.view?.hidden || deferredPins.has(item.id);
  const hiddenCount = pins.filter(isHidden).length;
  const visiblePins = pins.filter(item => !isHidden(item) && normalizePinView(item.view).group === group);
  const pinElements = [...visiblePins].reverse().map(item => <CapturePin key={item.id} workspace={workspace} record={item} english={english} onEdit={edit} onHidden={(value, destroyed) => { if (!destroyed) notify(t("贴图已隐藏，可从历史与贴图恢复", "Image hidden. Restore it from Pins and history."), false, value.id); }} />);

  return <>
    {!native && !pending && !open && <section className="cap-root cap-pin-layer" aria-label={t("页面贴图", "Pinned images")}>{pinElements}</section>}
    {open && !pending && <section className={`cap-root cap-workspace${settings.theme === "dark" ? " cap-dark" : ""}`} aria-label={t("截图与贴图", "Capture and pin")}>
      <header className="cap-header"><div className="cap-brand"><span className="cap-brand-mark"><CaptureIcon name="capture" /></span><div><strong>XChat <span style={{ fontWeight: 400 }}>{t("截图", "Capture")}</span></strong><small>CAPTURE & KEEP</small></div></div><div className="cap-header-spacer" /><button className="cap-top-button" type="button" aria-pressed={["pins", "history"].includes(panel)} onClick={() => setPanel("pins")}><CaptureIcon name="history" /><span>{hiddenCount ? t(`贴图 · ${hiddenCount} 张已隐藏`, `${hiddenCount} hidden pins`) : t("历史与贴图", "Pins and history")}</span></button><button className="cap-top-button" type="button" aria-pressed={panel === "settings"} onClick={() => setPanel("settings")}><CaptureIcon name="settings" /><span>{t("设置", "Settings")}</span></button><button className="cap-top-button cap-close-review" type="button" onClick={() => setOpen(false)}>{t("返回聊天", "Back to chat")}</button></header>
      <div className="cap-workbar"><Button primary icon="capture" disabled={busy} onClick={() => void run({ type: "capture.start" })}>{t("开始截图", "Capture")}<kbd>{captureShortcut || (settings.preset === "classic" ? "F1" : "Ctrl+Shift+A")}</kbd></Button><Button icon="pin" disabled={busy} onClick={() => void paste()}>{t("贴图", "Paste image")}</Button><Button onClick={() => { setSourceError(""); setSource({ kind: "text", text: "", color: "#18AC71", file: null }); }}>{t("更多来源", "More sources")}</Button><Button icon="pen" onClick={() => void whiteboard()}>{t("白板", "Whiteboard")}</Button><span className="cap-workbar-note"><b>{t(`${visiblePins.length} 张贴图`, `${visiblePins.length} pins`)}</b></span></div>
      <div className="cap-desktop"><div className="cap-workspace-intro" hidden={!native && visiblePins.length > 0}><CaptureIcon name="capture" /><h2>{t("捕捉重点，随手留存", "Capture what matters")}</h2><p>{t(native ? "截图、文字和参考图，都可以贴在桌面。隐藏的贴图可从右侧找回。" : "截图、文字和参考图，都可以贴在当前页面。隐藏的贴图可从右侧找回。", native ? "Keep captures, notes and references on your desktop. Restore hidden images from the panel." : "Keep captures, notes and references on this page. Restore hidden images from the panel.")}</p></div>
        {!native && <div className="cap-pin-layer cap-workspace-pins" aria-label={t("页面贴图", "Pinned images")}>{pinElements}</div>}
        <aside className="cap-panel" aria-label={panel === "settings" ? t("截图设置", "Capture settings") : t("历史与贴图", "Pins and history")}><header className="cap-panel-head"><div><h2>{panel === "settings" ? t("截图设置", "Capture settings") : t("历史与贴图", "Pins and history")}</h2><small>{panel === "settings" ? t("应用于当前设备上的截图与贴图。", "Applies to capture and pin on this device.") : t("隐藏可恢复，销毁会移除贴图。", "Hidden pins can be restored; destroyed pins are removed.")}</small></div></header><div className="cap-panel-body">
          {panel === "settings" ? <>
            <label className="cap-field"><span>{t("快捷键预设", "Shortcut preset")}</span><select value={settings.preset === "classic" ? "classic" : "xchat"} onChange={event => void setSetting("preset", event.target.value)}><option value="classic">{t("经典 · F1 / F3", "Classic · F1 / F3")}</option><option value="xchat">XChat · Ctrl+Shift+A / F3</option></select><small>{t(native ? "F3 打开贴图管理，可找回隐藏或穿透的贴图。" : "按键仅在当前页面获得焦点时有效。F3 打开贴图管理。", native ? "F3 opens Pins to recover hidden or click-through images." : "Shortcuts work while this page is focused. F3 opens Pins.")}</small></label>
            <label className="cap-field"><span>{t("开始前延时", "Capture delay")}</span><select value={settings.delay} onChange={event => void setSetting("delay", Number(event.target.value))}>{[0, 3, 5].map(value => <option key={value} value={value}>{value ? t(`${value} 秒`, `${value} seconds`) : t("立即开始", "No delay")}</option>)}</select></label>
            <div className="cap-preset-fields">{[["presetW", "预设宽度 px", "Preset width px"], ["presetH", "预设高度 px", "Preset height px"]].map(([key, zh, en]) => <label key={key} className="cap-field"><span>{t(zh, en)}</span><input type="number" min="0" max="16384" value={settings[key] || 0} onChange={event => void setSetting(key, Math.min(16384, Math.max(0, Number(event.target.value))))} /></label>)}</div><small className="cap-manager-status">{t("宽高均为正数时生效，0 表示自由框选。", "Set both dimensions to use a preset. Zero allows free selection.")}</small>
            {settingToggle("detect", "自动识别区域", "Detect regions", native ? "识别当前屏幕中可获取边界的窗口" : "使用屏幕来源提供的可用边界", native ? "Use available window boundaries on this screen" : "Use boundaries available from the shared source")}
            {settingToggle("magnifier", "显示像素放大镜", "Pixel magnifier", "框选时查看像素位置与色值", "Inspect pixel positions and colors while selecting")}
            {settingToggle("captureCursor", "包含鼠标指针", "Include mouse pointer", cursorSupported === null ? "正在检查当前截图环境" : cursorSupported ? "在截图中保留实际鼠标指针" : "当前截图环境不支持控制鼠标指针", cursorSupported === null ? "Checking capture support" : cursorSupported ? "Include the actual mouse pointer in captures" : "This capture environment cannot control the mouse pointer", cursorSupported !== true)}
            {settingToggle("remember", "重新打开时恢复贴图", "Restore pins on startup", "贴图与可编辑历史保存在本机", "Pins and editable history are stored on this device")}
            <label className="cap-field"><span>{t("历史上限", "History limit")}</span><select value={settings.limit} onChange={event => void setSetting("limit", Number(event.target.value))}>{[6, 12, 20].map(value => <option key={value} value={value}>{t(`${value} 条`, `${value} items`)}</option>)}</select></label>
            <label className="cap-field"><span>{t("界面外观", "Appearance")}</span><select value={settings.theme} onChange={event => void setSetting("theme", event.target.value)}><option value="light">{t("浅色", "Light")}</option><option value="dark">{t("深色", "Dark")}</option></select></label><div className="cap-note">{t("文字：回车换行，点击外部完成，拖动边框移动。工具滚轮调节大小；贴图滚轮缩放，Ctrl 加滚轮调透明度。", "Text: Enter inserts a line; click outside to finish; drag its border to move. Scroll to change tool size or pin zoom. Ctrl+scroll adjusts pin opacity.")}</div>
          </> : <>
            <div className="cap-tabs"><button type="button" className={panel === "pins" ? "active" : ""} onClick={() => setPanel("pins")}>{t("贴图", "Pins")} {pins.length}</button><button type="button" className={panel === "history" ? "active" : ""} onClick={() => setPanel("history")}>{t("截图历史", "History")} {history.length}</button></div>
            {panel === "pins" && <><div className="cap-filter">{["默认", "设计参考"].map(value => <button key={value} type="button" className={group === value ? "active" : ""} onClick={() => void setSetting("group", value)}>{t(value, value === "默认" ? "Default" : "References")}</button>)}</div><div className="cap-manager-actions"><Button small disabled={busy} onClick={() => void groupAction("show")}>{t("显示本组", "Show group")}</Button><Button small disabled={busy} onClick={() => void groupAction("hide")}>{t("隐藏本组", "Hide group")}</Button><Button small disabled={busy} onClick={() => void groupAction("recover")}>{t("找回屏外贴图", "Recover off-screen pins")}</Button></div>{native && groupPins.some(item => item.view?.through) && <div className="cap-note warning cap-recover-interaction">{t("本组有贴图正在使用鼠标穿透。", "Some pins in this group ignore mouse input.")}<div className="cap-manager-actions"><Button small onClick={() => void groupAction("through")}>{t("恢复鼠标交互", "Restore mouse input")}</Button></div></div>}</>}
            <div className="cap-cards">{(panel === "history" ? history : groupPins).map(item => <article className="cap-card" key={item.id} data-capture-record={item.id}><div className="cap-card-preview"><img src={item.data_url} alt={item.title || t("截图", "Capture")} /><span className="cap-tag">{panel === "history" ? t("历史", "History") : isHidden(item) ? t("已隐藏", "Hidden") : item.view?.through && native ? t("穿透中", "Click-through") : t("显示中", "Visible")}</span></div><div className="cap-card-content"><b>{item.title || t("截图", "Capture")}</b><small>{item.width} × {item.height} px</small><div className="cap-card-actions">{panel === "history" ? <><button type="button" onClick={() => void createPin(item).catch(cause => notify(cause.message, true))}>{t("贴图", "Pin")}</button><button type="button" onClick={() => edit(item)}>{t("回放", "Replay")}</button></> : <><button type="button" onClick={() => void locate(item)}>{isHidden(item) ? t("恢复", "Restore") : t("定位", "Locate")}</button><button type="button" onClick={() => void run({ type: "capture.pin.close", pinId: item.id, destroy: false })}>{t("隐藏", "Hide")}</button></>}</div></div></article>)}{!(panel === "history" ? history : groupPins).length && <div className="cap-empty">{panel === "history" ? t("完成一次截图后，记录会出现在这里。", "Completed captures will appear here.") : t("这个分组还没有贴图。", "No pins in this group yet.")}</div>}</div><div className="cap-note">{panel === "history" ? t("截图记录保留原始画面、选区与标注，可回放后继续修改文字。", "History keeps the original image, selection and annotations so text remains editable.") : t("在贴图右键菜单中可以切换分组、窗口阴影与显示方式。", "Right-click a pin to change its group, shadow and display.")}</div>
          </>}
        </div></aside>
      </div><footer className="cap-bottom"><span><i />{t(native ? "本机截图资料库" : "当前浏览器资料库", native ? "Local capture library" : "Browser capture library")}</span><span>{t("F3 贴图管理 · 右键更多操作", "F3 Pins · Right-click for more actions")}</span></footer>
      {source && <div className="cap-dialog-backdrop cap-source-dialog" onPointerDown={event => { if (event.target === event.currentTarget && !busy) setSource(null); }}><section className="cap-dialog" role="dialog" aria-modal="true" aria-label={t("从内容创建贴图", "Create pin from content")} onKeyDown={event => { if (event.key === "Escape" && !event.isComposing && !busy) { event.preventDefault(); event.stopPropagation(); setSource(null); return; } if (event.key === "Tab") { const nodes = [...event.currentTarget.querySelectorAll("button,input,textarea,select")].filter(node => !node.disabled); const first = nodes[0], last = nodes[nodes.length - 1]; if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); } else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); } } }}><div className="cap-dialog-head"><h2>{t("从内容创建贴图", "Create pin from content")}</h2><button type="button" className="cap-tool" aria-label={t("关闭", "Close")} disabled={busy} onClick={() => setSource(null)}><CaptureIcon name="close" /></button></div><p>{t("输入文字、颜色，或选择本地图片。", "Enter text or a color, or choose a local image.")}</p><div className="cap-paste-options">{[["text", "文字", "Text"], ["color", "颜色", "Color"], ["file", "图片文件", "Image file"]].map(([kind, zh, en]) => <button type="button" key={kind} disabled={busy} className={source.kind === kind ? "active" : ""} onClick={() => { setSource({ ...source, kind }); setSourceError(""); }}>{t(zh, en)}</button>)}</div>
        {source.kind === "text" ? <label className="cap-field"><span>{t("文字内容", "Text")}</span><textarea ref={sourceInput} value={source.text} onChange={event => setSource({ ...source, text: event.target.value })} placeholder={t("输入需要留存的文字…", "Enter a note…")} /></label> : source.kind === "color" ? <label className="cap-field"><span>{t("颜色值", "Color value")}</span><input ref={sourceInput} type="text" value={source.color} onChange={event => setSource({ ...source, color: event.target.value })} placeholder="#18AC71" /></label> : <label className="cap-field"><span>{t("本地图片", "Local image")}</span><input ref={sourceInput} type="file" accept="image/png,image/jpeg,image/webp,image/gif,image/bmp" onChange={event => setSource({ ...source, file: event.target.files?.[0] || null })} /><small>{t("支持 PNG、JPEG、WebP、BMP 和 GIF 静态帧。", "PNG, JPEG, WebP, BMP and static GIF frames are supported.")}</small></label>}
        <div className="cap-dialog-error" role="alert">{sourceError}</div><div className="cap-dialog-foot"><Button disabled={busy} onClick={() => setSource(null)}>{t("取消", "Cancel")}</Button><Button primary icon="pin" disabled={busy} onClick={() => void createSource()}>{busy ? t("正在创建…", "Creating…") : t("创建贴图", "Create pin")}</Button></div></section></div>}
    </section>}
    {pending && <CaptureSurface key={pending.session_id} workspace={workspace} pending={pending} english={english} onClose={() => { setPending(current => current?.session_id === pending.session_id ? null : current); void load().catch(cause => notify(cause.message, true)); }} />}
    {countdown > 0 && <section className="cap-root cap-countdown-host" aria-label={t("截图倒计时", "Capture countdown")}><div className="cap-countdown"><strong aria-live="polite">{countdown}</strong><button type="button" onClick={() => void run({ type: "capture.cancel-start", sessionId: countdownSession.current })}>{t("取消", "Cancel")}</button></div></section>}
    {toast && <section className="cap-root cap-workspace-toast"><div className={`cap-toast show${toast.error ? " error" : ""}`} role={toast.error ? "alert" : "status"}><span>{toast.message}</span>{toast.restoreId && <button type="button" onClick={() => { void run({ type: "capture.pin.restore", pinId: toast.restoreId }); setToast(null); }}>{t("恢复", "Restore")}</button>}</div></section>}
  </>;
}
