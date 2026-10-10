export const VOICE_MIN_MS = 1000;
export const VOICE_MAX_MS = 60_000;
export const VOICE_MAX_BYTES = 8 * 1024 * 1024;

export function voiceRecordingSupport(env = globalThis) {
  if (!env.navigator?.mediaDevices?.getUserMedia) return "需要安全连接（HTTPS 或 localhost）与麦克风权限";
  if (!env.MediaRecorder) return "当前浏览器不支持录音";
  return "";
}

export function voiceError(error) {
  if (["NotAllowedError", "SecurityError"].includes(error?.name)) return "麦克风权限未开启，请允许麦克风后重试";
  if (error?.name === "NotFoundError") return "没有找到麦克风，请连接后重试";
  if (error?.name === "NotReadableError") return "麦克风暂时不可用，请检查是否被其他应用占用";
  return error?.message || String(error || "录音失败，请重试");
}

// A generation owns its permission request, tracks, chunks and stop event. In
// particular, a permission result arriving after cancel can never start recording.
export function createVoiceRecorder(options = {}) {
  const env = options.env || globalThis;
  const now = options.now || (() => performance.now());
  const timer = options.setInterval || setInterval, clearTimer = options.clearInterval || clearInterval;
  let state = { phase: "idle", duration_ms: 0 }, epoch = 0, pending = false, destroyed = false;
  let stream, recorder, interval, startedAt = 0, chunks = [], bytes = 0, finishPromise, resolveFinish;
  const emit = (change) => { state = { ...state, ...change }; if (!destroyed) options.onChange?.(state); };
  const stopTracks = () => { stream?.getTracks().forEach(track => track.stop()); stream = null; };
  const clear = () => { if (interval != null) clearTimer(interval); interval = null; };
  const duration = () => Math.min(VOICE_MAX_MS, Math.max(0, Math.floor(now() - startedAt)));
  const ready = (result, token) => {
    if (token !== epoch || destroyed) { if (result?.file_path) options.discardNative?.(result); return; }
    clear(); stopTracks();
    const length = state.duration_ms;
    if (length < VOICE_MIN_MS || (!result?.blob?.size && !result?.file_path)) {
      if (result?.file_path) void options.discardNative?.(result);
      emit({ phase: "error", error: length < VOICE_MIN_MS ? "录音时间太短，请至少录制 1 秒" : "录音没有内容，请重试" });
      resolveFinish?.(null); return;
    }
    const recording = { ...result, duration_ms: length, recording_id: state.recording_id };
    emit({ phase: "ready", recording, error: "" }); resolveFinish?.(recording);
  };
  async function finish() {
    if (state.phase === "ready") return state.recording;
    if (finishPromise) return finishPromise;
    if (state.phase !== "recording") return null;
    const token = epoch;
    emit({ phase: "stopping", duration_ms: duration() }); clear();
    finishPromise = new Promise(resolve => { resolveFinish = resolve; });
    if (options.native) {
      Promise.resolve(options.stopNative(false)).then(result => {
        if (result?.status !== "ok") throw new Error("录音失败，请重试");
        ready({ file_path: result.path || result.file_path, mime_type: result.mime_type || "audio/mp4" }, token);
      }).catch(error => { if (token === epoch) { emit({ phase: "error", error: voiceError(error) }); resolveFinish?.(null); } });
    } else {
      try { recorder.stop(); } catch (error) { stopTracks(); emit({ phase: "error", error: voiceError(error) }); resolveFinish?.(null); }
      stopTracks();
    }
    return finishPromise;
  }
  async function cancel() {
    const previous = state, wasRecording = state.phase === "recording";
    const token = ++epoch; clear();
    if (recorder?.state === "recording") recorder.stop();
    stopTracks();
    if (options.native && wasRecording) await options.stopNative(true).catch(() => {});
    if (previous.recording?.file_path) await options.discardNative?.(previous.recording);
    if (token !== epoch) return;
    resolveFinish?.(null); finishPromise = null; resolveFinish = null; recorder = null; chunks = [];
    emit({ phase: "idle", duration_ms: 0, recording: null, error: "" });
  }
  async function start() {
    if (pending || destroyed || !["idle", "error"].includes(state.phase)) return;
    const token = ++epoch; pending = true; chunks = []; bytes = 0; finishPromise = null;
    emit({ phase: "requesting", error: "", duration_ms: 0, recording: null, recording_id: env.crypto.randomUUID() });
    try {
      if (options.native) {
        const result = await options.startNative();
        if (token !== epoch || destroyed) { await options.stopNative(true); return; }
        if (result?.status !== "recording") throw new Error(result?.status === "permission_denied" ? "麦克风权限未开启，请允许后重试" : result?.message || "无法开始录音");
      } else {
        const unsupported = voiceRecordingSupport(env); if (unsupported) throw new Error(unsupported);
        const acquired = await env.navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true }, video: false });
        if (token !== epoch || destroyed) { acquired.getTracks().forEach(track => track.stop()); return; }
        stream = acquired;
        const mime = ["audio/webm;codecs=opus", "audio/ogg;codecs=opus", "audio/mp4"].find(type => env.MediaRecorder.isTypeSupported(type));
        if (!mime) throw new Error("当前环境没有可用的语音编码器");
        recorder = new env.MediaRecorder(stream, { mimeType: mime, audioBitsPerSecond: 64000 });
        const activeRecorder = recorder;
        recorder.ondataavailable = event => {
          if (token !== epoch || !event.data?.size) return;
          bytes += event.data.size;
          if (bytes > VOICE_MAX_BYTES) { void cancel().then(() => emit({ phase: "error", error: "语音文件过大，已停止录制" })); return; }
          chunks.push(event.data);
        };
        recorder.onstop = () => {
          if (token !== epoch) return;
          if (state.phase === "recording") emit({ duration_ms: duration() });
          ready({ blob: new Blob(chunks, { type: activeRecorder.mimeType.split(';')[0] }) }, token);
        };
        recorder.onerror = event => { if (token !== epoch) return; const error = voiceError(event.error); void cancel().then(() => emit({ phase: "error", error })); };
        recorder.start(250);
      }
      startedAt = now(); emit({ phase: "recording", duration_ms: 0 });
      interval = timer(() => { if (duration() >= VOICE_MAX_MS) void finish(); else emit({ duration_ms: duration() }); }, 100);
    } catch (error) {
      stopTracks(); clear(); if (token === epoch) emit({ phase: "error", error: voiceError(error) });
    } finally { pending = false; }
  }
  return { start, finish, cancel, snapshot: () => state, destroy() { destroyed = true; void cancel(); } };
}
