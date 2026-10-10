const stop = stream => stream?.getTracks().forEach(track => track.stop());
const liveTrack = stream => stream?.getAudioTracks().find(track => track.readyState !== "ended");

function voiceError(message, code) {
  return Object.assign(new Error(message), {code});
}

// WKWebView only exposes mediaDevices when its embedding app is allowed to capture
// audio. Check this separately from permission denial: retrying getUserMedia cannot
// repair a macOS build without NSMicrophoneUsageDescription.
export function microphoneCapabilityError(environment = globalThis) {
  if (environment.isSecureContext === false) {
    return voiceError("当前页面无法使用麦克风，请使用桌面客户端、HTTPS 或本机 localhost 地址", "insecure_context");
  }
  if (typeof environment.navigator?.mediaDevices?.getUserMedia !== "function") {
    const platform = environment.navigator?.userAgentData?.platform || environment.navigator?.platform || "";
    const mac = /mac/i.test(platform);
    return voiceError(mac && environment.window?.__TAURI__
      ? "当前 macOS 客户端无法访问麦克风，请更新客户端并检查系统设置中的麦克风权限"
      : "当前环境不支持麦克风采集，请使用最新桌面客户端或支持语音的浏览器", "capture_unavailable");
  }
  return null;
}

export function microphoneError(error) {
  if (error?.code && ["insecure_context", "capture_unavailable", "no_audio_track"].includes(error.code)) return error;
  const messages = {
    NotAllowedError: "麦克风权限未开启，请在系统设置和客户端权限提示中允许使用麦克风后重试",
    SecurityError: "当前环境禁止访问麦克风，请检查系统和浏览器的麦克风权限",
    NotFoundError: "未找到可用的麦克风，请连接麦克风后重试",
    DevicesNotFoundError: "未找到可用的麦克风，请连接麦克风后重试",
    NotReadableError: "麦克风暂时无法读取，请检查设备连接或其他应用占用后重试",
    TrackStartError: "麦克风暂时无法读取，请检查设备连接或其他应用占用后重试",
    OverconstrainedError: "所选麦克风不可用，请重新选择麦克风",
    AbortError: "麦克风启动被中断，请重试",
  };
  return voiceError(messages[error?.name] || `麦克风不可用：${String(error?.message || error || "设备未提供错误信息")}`, error?.name || "capture_failed");
}

function current(media, epoch, call) {
  return !media.closed && epoch === media.audioEpoch && media.session.voice.stage === "active" && media.session.voice.id === call;
}

function replaceTrack(media, track, valid) {
  const sender = media.voice?.sender;
  if (!sender) return Promise.resolve();
  // Serialize sender changes so an old hangup/device replacement cannot finish
  // after a new call and detach its microphone.
  const operation = (media.voiceAttachment || Promise.resolve()).catch(() => {}).then(() => {
    if (!media.closed && media.voice?.sender === sender && valid()) return sender.replaceTrack(track());
  });
  media.voiceAttachment = operation.catch(() => {});
  return operation;
}

export function attachRemoteVoice(media) {
  return replaceTrack(media, () => media.session.voice.stage === "active" && media.microphoneCall === media.session.voice.id ? liveTrack(media.microphone) || null : null, () => true);
}

export function syncRemoteVoice(media, deviceId = media.inputDevice) {
  if (media.closed) return Promise.resolve();
  const call = media.session.voice.id;
  if (media.session.voice.stage !== "active") {
    const epoch = ++media.audioEpoch;
    const previous = media.microphone;
    media.microphone = null;
    media.microphoneCall = null;
    stop(previous);
    media.changed();
    return replaceTrack(media, () => null, () => epoch === media.audioEpoch).catch(() => {});
  }
  // Polling may observe a redial without the intermediate idle update. Never keep
  // the previous call's source live while the new call waits for its microphone.
  if (media.microphone && media.microphoneCall !== call) {
    const previous = media.microphone;
    media.microphone = null;
    media.microphoneCall = null;
    stop(previous);
    media.changed();
  }
  if (liveTrack(media.microphone) && media.microphoneCall === call && deviceId === media.inputDevice) return Promise.resolve();
  if (media.voiceAcquisition?.call === call && media.voiceAcquisition.deviceId === deviceId && media.voiceAcquisition.epoch === media.audioEpoch) {
    return media.voiceAcquisition.promise;
  }
  const epoch = ++media.audioEpoch;
  const acquisition = {call, deviceId, epoch};
  media.voiceAcquisition = acquisition;
  acquisition.promise = (async () => {
    let stream;
    try {
      const unavailable = microphoneCapabilityError();
      if (unavailable) throw unavailable;
      stream = await navigator.mediaDevices.getUserMedia({
        audio: {echoCancellation: true, noiseSuppression: true, autoGainControl: true, ...(deviceId ? {deviceId: {exact: deviceId}} : {})},
        video: false,
      });
      if (!current(media, epoch, call)) {stop(stream); return;}
      const track = liveTrack(stream);
      if (!track) throw voiceError("系统没有提供可用的麦克风音轨，请检查麦克风连接后重试", "no_audio_track");
      // Keep a newly attached source silent until replacement completes. A mute
      // action can arrive while replaceTrack is pending.
      track.enabled = false;
      await replaceTrack(media, () => track, () => current(media, epoch, call));
      if (!current(media, epoch, call)) {stop(stream); return;}
      track.enabled = !media.session.voice.local_muted;
      const previous = media.microphone;
      media.microphone = stream;
      media.microphoneCall = call;
      media.inputDevice = deviceId;
      stop(previous);
      track.addEventListener("ended", () => {
        if (!media.closed && media.microphone === stream && media.session.voice.id === call && media.session.voice.stage === "active") {
          void media.voiceFailed(new Error("麦克风已断开，可重新发起语音"), call);
        }
      }, {once: true});
      media.changed({voiceError: ""});
    } catch (error) {
      stop(stream);
      if (!current(media, epoch, call)) return;
      const failure = microphoneError(error);
      // A failed device switch must not tear down a working call.
      if (liveTrack(media.microphone) && media.microphoneCall === call) media.changed({voiceError: failure.message});
      else await media.voiceFailed(failure, call);
    } finally {
      if (media.voiceAcquisition === acquisition) media.voiceAcquisition = null;
    }
  })();
  return acquisition.promise;
}
