import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createVoiceRecorder, voiceRecordingSupport } from "./voice-recorder.js";
import { mediaPositionKey } from "./media-playback.js";
import "./voice-message.css";

function VoiceIcon({ name = "voice" }) {
  return <svg viewBox="0 0 24 24" aria-hidden="true"><g fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">{name === "close" ? <path d="m6 6 12 12M6 18 18 6"/> : name === "send" ? <path d="M12 20V4m-6 6 6-6 6 6"/> : name === "pause" ? <path d="M8 5v14M16 5v14"/> : <><circle cx="12" cy="12" r="9"/><path d="M9 10a3 3 0 0 1 0 4m3-6a6 6 0 0 1 0 8m3-10a9 9 0 0 1 0 12"/></>}</g></svg>;
}

export function VoiceComposer({ workspace, conversationId, native, playback, onActive, disabled, children }) {
  const [state, setState] = useState({ phase: "idle", duration_ms: 0 });
  const [sending, setSending] = useState(false);
  const [error, setError] = useState("");
  const controller = useRef(null), sendLock = useRef(false), focus = useRef(null);
  const active = !["idle", "error"].includes(state.phase);
  useEffect(() => { onActive(active); return () => onActive(false); }, [active, onActive]);
  useEffect(() => {
    const unwrap = async action => { const result = await workspace.dispatch(action); if (!result.ok) throw new Error(result.error.message); return result.data; };
    const recorder = createVoiceRecorder({ native, onChange: setState,
      startNative: () => unwrap({ type: "voice.record.start" }),
      stopNative: cancelled => unwrap({ type: "voice.record.stop", cancelled }),
      discardNative: recording => workspace.dispatch({ type: "voice.record.discard", recording }),
    });
    controller.current = recorder; setState(recorder.snapshot()); setError("");
    const close = () => { if (!sendLock.current) void recorder.cancel(); };
    const hidden = () => { if (document.hidden && recorder.snapshot().phase === "recording") void recorder.finish(); };
    addEventListener("pagehide", close); document.addEventListener("visibilitychange", hidden);
    return () => { recorder.destroy(); removeEventListener("pagehide", close); document.removeEventListener("visibilitychange", hidden); };
  }, [workspace, conversationId, native]);
  const cancel = async () => { if (sendLock.current) return; await controller.current.cancel(); setError(""); focus.current?.focus(); };
  const send = async () => {
    if (sendLock.current) return;
    sendLock.current = true; setSending(true); setError("");
    const recorder = controller.current;
    try {
      const recording = await recorder.finish();
      if (!recording) return;
      const result = await workspace.dispatch({ type: "message.sendVoice", conversationId, recording });
      if (!result.ok) { setError(result.error.message); return; }
      await recorder.cancel();
    } finally { sendLock.current = false; setSending(false); }
  };
  const unsupported = native ? "" : voiceRecordingSupport();
  const seconds = Math.floor(state.duration_ms / 1000);
  return <div className={`voice-compose${active ? " active" : ""}`} onKeyDown={event => {
    if (event.key === "Escape" && active) { event.preventDefault(); event.stopPropagation(); void cancel(); }
  }}>
    {active ? <div className={`voice-recorder${state.phase === "ready" ? " ready" : ""}`}>
      <button type="button" className="voice-cancel" onClick={cancel} disabled={sending} title="取消录制" aria-label="取消录制"><VoiceIcon name="close"/></button>
      <div className="voice-record-pill"><span className="voice-record-time">0:{String(seconds).padStart(2, "0")}</span><span className="voice-wave" aria-hidden="true">{[1,3,5,7,6,4,2,3,1].map((n,i) => <i key={i} style={{ "--i": n }}/>)}</span><button type="button" className="voice-record-send" onClick={send} disabled={sending || state.duration_ms < 1000 || !["recording", "ready"].includes(state.phase)} title="发送语音" aria-label="发送语音"><VoiceIcon name="send"/></button></div>
    </div> : <><button ref={focus} type="button" className="icon-button voice-entry" disabled={disabled || !!unsupported} aria-label="录制语音消息" title={unsupported || "语音消息"} onClick={() => { playback.pauseAll(); setError(""); void controller.current.start(); }}><VoiceIcon/></button>{children}</>}
    {(active || error || state.error) && <span className={`voice-record-hint${error || state.error ? " error" : ""}`} role="status">{error || state.error || (sending ? "正在发送语音…" : state.phase === "requesting" ? "等待麦克风授权…" : state.phase === "stopping" ? "正在保存录音…" : state.phase === "ready" ? "录制已结束，可发送或取消" : "正在录音 · 最长 60 秒")}</span>}
  </div>;
}

export function VoiceBubble({ source, message, playback, onError }) {
  const player = useRef(null);
  const key = mediaPositionKey(message.conversation_id, message);
  const heardKey = `xchat.voice.heard:${key}`;
  const [heard, setHeard] = useState(() => { try { return localStorage.getItem(heardKey) === "1"; } catch { return false; } });
  const [playing, setPlaying] = useState(false), [position, setPosition] = useState(0);
  const [duration, setDuration] = useState((message.voice?.duration_ms || message.duration_ms || 1000) / 1000);
  useLayoutEffect(() => playback.register(player.current, key), [key, playback]);
  const toggle = async () => {
    if (!player.current.paused) { player.current.pause(); return; }
    try { await player.current.play(); } catch { onError(); }
  };
  return <div className="voice-message" data-media-kind="voice">
    <audio ref={player} preload="metadata" src={source.url}
      onLoadedMetadata={event => { if (Number.isFinite(event.currentTarget.duration)) setDuration(event.currentTarget.duration); playback.restore(event.currentTarget); }}
      onPlay={event => { playback.play(event.currentTarget); setPlaying(true); setHeard(true); try { localStorage.setItem(heardKey, "1"); } catch {} }}
      onPause={event => { setPlaying(false); playback.remember(event.currentTarget); }}
      onTimeUpdate={event => { setPosition(event.currentTarget.currentTime); playback.remember(event.currentTarget); }}
      onEnded={event => { setPlaying(false); setPosition(0); playback.remember(event.currentTarget); }} onError={onError}/>
    <button className={`voice-bubble${playing ? " playing" : ""}`} type="button" data-media-control style={{ "--voice-width": `${Math.min(270, 130 + duration * 3)}px` }} onClick={toggle} aria-label={`${playing ? "暂停" : "播放"}语音，${Math.ceil(duration)} 秒`} aria-pressed={playing}>
      <VoiceIcon name={playing ? "pause" : "voice"}/><span>{Math.ceil(duration)}″</span><span className="voice-play-track"><i style={{ width: `${Math.min(100, position / duration * 100)}%` }}/></span>{!message.own && !heard && <i className="voice-unheard" aria-label="尚未播放"/>}
    </button><span className="voice-play-copy">{playing ? `正在播放 ${Math.floor(position)} 秒` : position > 0 ? "已暂停 · 点击继续" : heard || message.own ? "点击播放" : "点击播放 · 未播放"}</span>
  </div>;
}
