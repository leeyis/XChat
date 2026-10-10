import {RemoteFloatingPanel,RemoteIcon,RemoteTool,SharingControls} from './RemoteControls.jsx';
import {remoteDuration} from './remote-model.js';

export default function RemoteSessionView({remote,name,localName='我',busy,scale,setScale,chat,setChat,draft,setDraft,messages,conversation,onSendChat,onEnd,onAction,onQuality,onScreen,onControl,onAudio,onFull,fullscreen,memory,releaseInput,children}) {
  const s=remote.session,host=s.local_host,control=Boolean(s.grant),paused=s.paused,voice=s.voice;
  const quality=({auto:'自动',fluent:'优先流畅',clear:'优先清晰'})[s.quality.preset]||'自动';
  const revoke=()=>{releaseInput?.();onAction({type:host?'control':'release_control',...(host?{allow:false}:{})});};
  const pause=()=>onAction({type:'pause',paused:!paused});
  const toolbar=({grip,hide,hidden}={})=><div className={`ra-toolbar ${host?'':'ra-panel-tools'}`} hidden={hidden}>
    {grip}<span className="ra-screen-label" title={s.screen?.name}><RemoteIcon/> {s.screen?.name||'共享屏幕'}</span>
    {host&&<RemoteTool icon="screen" disabled={busy} onClick={onScreen}>切换屏幕</RemoteTool>}
    <span className="ra-tool-divider"/><label className="ra-scale-label">显示<select aria-label="远程画面缩放" value={scale} onChange={e=>setScale(e.target.value)}><option value="fit">适应窗口</option><option value="actual">原始比例 100%</option></select></label>
    <RemoteTool icon="tune" onClick={onQuality}>{quality}</RemoteTool><RemoteTool icon={fullscreen?'shrink':'expand'} onClick={onFull}>{fullscreen?'退出全屏':'全屏'}</RemoteTool><span className="ra-toolbar-spacer"/>
    {!host&&!control&&!paused&&<RemoteTool icon="control" className="primary" disabled={busy||!s.native_host||s.control_requested} onClick={()=>onAction({type:'request_control'})}>{s.control_requested?'等待授权':'申请控制'}</RemoteTool>}
    {!host&&control&&<RemoteTool icon="control" disabled={busy} onClick={revoke}>释放控制</RemoteTool>}
    {host&&!control&&!paused&&s.native_host&&<RemoteTool icon="control" className="primary" disabled={busy} onClick={onControl}>{s.control_requested?'处理控制请求':'允许对方控制'}</RemoteTool>}
    <RemoteTool icon="chat" aria-pressed={chat} onClick={()=>setChat(!chat)}>沟通</RemoteTool>{hide}
  </div>;
  const voiceActive=voice.stage==='active',ringing=voice.stage==='ringing';
  const voiceTitle=voiceActive?(remote.microphone?'双向通话':'正在接通麦克风…'):ringing?(voice.local_caller?'正在呼叫对方':'对方邀请你语音通话'):'边说边协作';
  return <>
    <header className="ra-session-head"><div className="ra-session-identity"><span className="ra-avatar small">{name.slice(0,1)}</span><div><h1>{host?(paused?'我的屏幕已暂停共享':'我的屏幕正在共享'):`${name}的远程桌面`}</h1><p><span className="ra-online"/>{localName} · {host?'共享方':'协助方'}<span className="ra-dot">·</span><span>{remoteDuration(s.started_at)}</span></p></div></div>
      <div className="ra-session-head-actions"><span className={`ra-session-permission ${control?'control':''}`}><RemoteIcon name={control?'control':'screen'}/>{paused?'已暂停':control?'允许控制':'仅查看'}</span><RemoteTool icon="close" className="danger" onClick={onEnd}>结束协助</RemoteTool></div>
    </header>
    {host&&toolbar()}
    <section className={`ra-voice-bar ${voiceActive?'active':''}`} aria-label="远程协助语音"><span className="ra-voice-emblem"><RemoteIcon name="phone"/></span><div className="ra-voice-summary"><strong>{voiceTitle}{voiceActive&&<span data-ra-voice-elapsed>{remoteDuration(voice.started_at)}</span>}</strong><small>{voiceActive?'双方可以直接说话，无需按住按钮':ringing?'等待接听，远程画面继续。':'语音与屏幕共享可分别操作'}</small></div>
      {voiceActive&&<div className="ra-voice-people">{[['我',voice.local_muted],[name,voice.peer_muted]].map(([label,muted],i)=><span key={i} className={muted?'muted':''}><RemoteIcon name={muted?'micOff':'mic'}/><b>{label}</b><small>{muted?'麦克风关闭':'麦克风开启'}</small></span>)}</div>}
      <div className="ra-voice-actions">{voiceActive?<><RemoteTool icon={voice.local_muted?'micOff':'mic'} aria-pressed={voice.local_muted} disabled={busy} onClick={()=>onAction({type:'muted',muted:!voice.local_muted})}>{voice.local_muted?'开启麦克风':'关闭麦克风'}</RemoteTool><RemoteTool icon={remote.speaker?'sound':'soundOff'} aria-pressed={!remote.speaker} onClick={()=>onAction({type:'speaker',enabled:!remote.speaker})}>{remote.speaker?'关闭扬声器':'开启扬声器'}</RemoteTool><RemoteTool icon="tune" onClick={onAudio}>音频设置</RemoteTool><RemoteTool icon="hangup" className="danger" disabled={busy} onClick={()=>onAction({type:'voice_end'})}>挂断语音</RemoteTool></>:ringing?<>{!voice.local_caller&&<RemoteTool icon="phone" className="primary" disabled={busy} onClick={()=>onAction({type:'voice_answer',id:voice.id,accepted:true})}>接通语音</RemoteTool>}<RemoteTool icon="hangup" disabled={busy} onClick={()=>onAction(voice.local_caller?{type:'voice_end'}:{type:'voice_answer',id:voice.id,accepted:false})}>{voice.local_caller?'取消呼叫':'暂不接听'}</RemoteTool></>:<RemoteTool icon="phone" disabled={busy} onClick={()=>onAction({type:'voice_invite'})}>发起语音通话</RemoteTool>}</div>
    </section>
    {(remote.voiceError||remote.audioBlocked)&&<p className="remote-audio-notice" role="status">{remote.voiceError||'浏览器等待你开启通话声音'}{remote.audioBlocked&&<RemoteTool onClick={()=>onAction({type:'play_audio'})}>开启声音</RemoteTool>}</p>}
    <div className={`ra-workspace remote-body ${chat?'with-chat':''}`}><main className="ra-canvas-wrap remote-stage">
      {!host&&<div className="ra-viewer-status remote-permission"><RemoteIcon name={control?'control':'screen'}/><span>{paused?'对方已暂停共享':control?'鼠标与键盘已获本次授权':'当前仅查看，操作需要对方同意'}</span></div>}
      <RemoteFloatingPanel key={s.id} session={s} memory={memory} busy={busy} onRevoke={revoke} onEnd={onEnd} beforeInteract={releaseInput}>
        {host?props=><SharingControls {...props} session={s} busy={busy} onRevoke={revoke} onPause={pause} onEnd={onEnd}/>:toolbar}
      </RemoteFloatingPanel>
      {children}
      <footer className="ra-connection-strip"><span><i className="ra-online"/>{remote.connectionState==='connected'?'局域网直连':'正在连接'}</span>{remote.metrics.rtt!=null&&<span>{remote.metrics.rtt} ms</span>}{remote.metrics.fps!=null&&<span>{Math.round(remote.metrics.fps)} FPS</span>}{remote.metrics.width&&<span>{remote.metrics.width} × {remote.metrics.height}</span>}<span className="ra-connection-right">{paused?'已暂停':quality} · {scale==='actual'?'100%':'适应窗口'}</span></footer>
    </main>
      {chat&&<aside className="ra-chat-panel remote-chat"><div className="ra-chat-head"><h3>会话沟通</h3><button type="button" className="ra-button ra-chat-close" aria-label="收起会话沟通" onClick={()=>setChat(false)}>×</button></div><div className="ra-chat-note"><RemoteIcon name="chat"/>协助时，继续把问题说清楚</div><div className="ra-chat-messages">{messages.slice(-40).map(m=><div key={m.client_message_id||m.id} className={`ra-chat-message ${m.own?'mine':''}`}><span>{m.own?'我':name}</span><p>{['text','quote'].includes(m.msg_type)?m.content:m.msg_type==='voice'?'[语音消息]':'[文件]'}</p>{m.own&&m.status==='failed'&&<small className="ra-message-error">发送失败，请回到聊天重试</small>}</div>)}</div><div className="ra-chat-compose"><textarea aria-label="协助消息" rows={3} placeholder="说一下你看到的问题…" value={draft} maxLength={2000} onChange={e=>setDraft(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'&&!e.shiftKey&&!e.nativeEvent.isComposing){e.preventDefault();onSendChat();}}}/><div><span>Enter 发送</span><RemoteTool icon="send" className="primary" disabled={busy||!draft.trim()||!conversation} onClick={onSendChat}>发送</RemoteTool></div></div></aside>}
    </div>
  </>;
}
