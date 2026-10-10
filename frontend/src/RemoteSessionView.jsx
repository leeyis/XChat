import {useEffect,useRef,useState} from 'react';
import {RemoteIcon,RemoteTool} from './RemoteControls.jsx';
import {remoteAccepted,remoteDuration} from './remote-model.js';
import './remote-viewer.css';

const number=(value,suffix='',digits=0)=>typeof value==='number'&&Number.isFinite(value)?`${value.toFixed(digits)}${suffix}`:'—';
const connectionLabel=remote=>remote.connectionState==='connected'
  ?remote.metrics?.path==='relay'?'中继连接':remote.metrics?.path==='direct'?'点对点直连':'已连接'
  :remote.connectionState==='failed'?'连接失败':remote.connectionState==='disconnected'?'正在重连':'正在连接';

function ConnectionInfo({remote,onClose}) {
  const [tab,setTab]=useState('performance');
  const session=remote.session,m=remote.metrics||{},host=m.hostInfo||session.host_info||{};
  const direct=m.path==='direct'?'P2P 直连':m.path==='relay'?'中继':'—';
  const protocol=m.protocol?m.protocol.toUpperCase():null;
  const rows=tab==='performance'?[
    ['接收处理延时',number(m.processingMs,' ms',1)],['帧率',number(m.fps,' FPS',1)],
    ['分辨率',m.width&&m.height?`${m.width} × ${m.height}`:'—'],['带宽占用',number(m.kbps,' Kbps')],
    ['丢包率',number(m.lossPercent,'%',1)],['解码延时',number(m.decodeMs,' ms',1)],
    ['抖动缓冲延时',number(m.jitterBufferMs,' ms',1)],
  ]:[
    ['主机名称',session.peer_name],['操作系统',(host.os_version||host.os)?[host.os_version||host.os,host.architecture].filter(Boolean).join(' · '):'未提供'],['处理器',host.cpu||'未提供'],
    ['内存',typeof host.memory_bytes==='number'?number(host.memory_bytes/1073741824,' GB',1):'未提供'],['显示器',session.screen?.name||'—'],
    ['原始分辨率',session.screen?.width&&session.screen?.height?`${session.screen.width} × ${session.screen.height}`:'—'],
  ];
  const details=tab==='performance'?[
    ['传输通道',protocol?`${protocol} · ${direct}`:direct],['编码方式',m.codec||'—'],
    ['编码器',m.hostEncoderImplementation?`${m.hostEncoderImplementation}${m.hostHardwareEncoder===true?' · 硬件':''}`:'未提供'],
    ['解码器',m.decoderImplementation?`${m.decoderImplementation}${m.mediaTransport==='dtls-sctp'?' · 硬件状态未提供':''}`:'未提供'],
    ['采集方式',m.hostCaptureBackend||'未提供'],['采集耗时',number(m.hostCaptureMs,' ms',1)],
    ['编码耗时',number(m.hostEncodeMs,' ms',1)],['媒体加密',m.mediaTransport==='dtls-sctp'?(m.dtlsState==='connected'?['DTLS',m.dtlsCipher].filter(Boolean).join(' · '):'—'):m.srtpCipher||(m.dtlsState==='connected'?'DTLS-SRTP':'—')],
  ]:[
    ['被控方 IP',m.remoteAddress||'未提供'],['主控方 IP',m.localAddress||'未提供'],
    ['客户端版本',host.client_version||'未提供'],['会话权限',session.grant?'屏幕、鼠标和键盘':'仅查看屏幕'],
    ['连接时长',remoteDuration(session.started_at)],
  ];
  return <aside className="remote-info-popover" aria-label="主机与连接信息" onKeyDown={event=>{if(event.key==='Escape'){event.stopPropagation();onClose();}}}>
    <div className="remote-info-head"><strong>主机与连接信息</strong><RemoteTool icon="close" className="quiet icon-only" aria-label="关闭信息" onClick={onClose}/></div>
    <div className="remote-info-tabs" role="tablist" aria-label="信息类别"><button role="tab" aria-selected={tab==='performance'} onClick={()=>setTab('performance')}>连接性能</button><button role="tab" aria-selected={tab==='host'} onClick={()=>setTab('host')}>远程主机</button></div>
    {tab==='performance'&&<div className="remote-latency"><RemoteIcon name="signal"/><span>网络延时</span><strong>{number(m.rtt)}</strong><small>ms</small></div>}
    <dl>{rows.map(([label,value])=><div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}</dl><hr/>
    <dl>{details.map(([label,value])=><div key={label}><dt>{label}</dt><dd title={String(value)}>{value}</dd></div>)}</dl>
    <p className="remote-info-note">当前会话实时统计。— 表示尚未测得；未暴露的信息显示“未提供”。接收处理延时不等于端到端画面延时。</p>
  </aside>;
}

export function RemoteVoiceBar({remote,name,busy,onAction,onAudio}) {
  const voice=remote.session?.voice;
  if(!voice)return null;
  const active=voice.stage==='active',ringing=voice.stage==='ringing';
  if(!active&&!ringing&&!remote.voiceError&&!remote.audioBlocked)return null;
  return <section className={`remote-chat-voice remote-session ${active?'active':''}`} aria-label="远程协助语音">
    <div className="remote-chat-voice-line"><RemoteIcon name={voice.local_muted?'micOff':'phone'}/><strong>{active?'语音通话中':ringing?voice.local_caller?'正在呼叫对方':'对方邀请你语音通话':'语音未接通'}</strong><span>{active?`${remoteDuration(voice.started_at)} · ${voice.local_muted?'麦克风已静音':remote.microphone?'双方已接通':'正在接通麦克风…'}`:ringing?`${name} · 远程画面继续`:'屏幕共享继续'}</span><i className="remote-flex"/>
      {active?<><RemoteTool icon={voice.local_muted?'micOff':'mic'} className="quiet" aria-pressed={voice.local_muted} disabled={busy} onClick={()=>onAction({type:'muted',muted:!voice.local_muted})}>{voice.local_muted?'取消静音':'静音'}</RemoteTool><RemoteTool icon={remote.speaker?'sound':'soundOff'} className="quiet icon-only" aria-label={remote.speaker?'关闭扬声器':'开启扬声器'} title={remote.speaker?'关闭扬声器':'开启扬声器'} onClick={()=>onAction({type:'speaker',enabled:!remote.speaker})}/><RemoteTool icon="tune" className="quiet icon-only" aria-label="音频设置" title="音频设置" onClick={onAudio}/><RemoteTool icon="hangup" className="danger quiet" disabled={busy} onClick={()=>onAction({type:'voice_end'})}>挂断</RemoteTool></>:ringing?<>{!voice.local_caller&&<RemoteTool icon="phone" className="primary" disabled={busy} onClick={()=>onAction({type:'voice_answer',id:voice.id,accepted:true})}>接听</RemoteTool>}<RemoteTool icon="hangup" className="quiet" disabled={busy} onClick={()=>onAction(voice.local_caller?{type:'voice_end'}:{type:'voice_answer',id:voice.id,accepted:false})}>{voice.local_caller?'取消':'暂不接听'}</RemoteTool></>:<RemoteTool icon="phone" className="quiet" disabled={busy} onClick={()=>onAction({type:'voice_invite'})}>重新呼叫</RemoteTool>}
    </div>
    {(remote.voiceError||remote.audioBlocked)&&<p className="remote-chat-audio-notice" role="status">{remote.voiceError||'浏览器等待你开启通话声音'}{remote.audioBlocked&&<RemoteTool className="quiet" onClick={()=>onAction({type:'play_audio'})}>开启声音</RemoteTool>}</p>}
  </section>;
}

export default function RemoteSessionView({remote,name,busy,scale,setScale,onEnd,onAction,onQuality,onChat,onPlayAudio,onFull,fullscreen,children}) {
  const [info,setInfo]=useState(false),infoButton=useRef(null),popover=useRef(null);
  const session=remote.session,m=remote.metrics||{},active=remoteAccepted(session);
  useEffect(()=>{if(!info)return;const outside=event=>{if(!popover.current?.contains(event.target)&&!infoButton.current?.contains(event.target))setInfo(false);};document.addEventListener('pointerdown',outside);return()=>document.removeEventListener('pointerdown',outside);},[info]);
  if(!session)return null;
  const control=Boolean(session.grant),quality=({auto:'自动画质',fluent:'流畅优先',clear:'清晰优先'})[session.quality?.preset]||'自动画质';
  return <>
    <header className="remote-viewer-heading"><RemoteIcon/><strong>{name}</strong><span>远程桌面 · XChat</span><RemoteTool icon="close" className="danger quiet" onClick={onEnd}>结束协助</RemoteTool></header>
    <div className="remote-viewer-tools"><label className="remote-screen-name"><RemoteIcon/><select aria-label="远程显示器" title="由共享方切换屏幕" value={session.screen?.id||''} disabled><option value={session.screen?.id||''}>{session.screen?.name||'共享屏幕'}</option></select></label><i className="remote-divider"/><label className="remote-scale-choice"><span>显示</span><select aria-label="远程画面缩放" value={scale} onChange={event=>setScale(event.target.value)}><option value="fit">适应窗口</option><option value="actual">原始大小</option></select></label>
      <RemoteTool icon="tune" className="quiet remote-quality-tool" disabled={!active||busy} onClick={onQuality}>{quality}</RemoteTool><RemoteTool icon={fullscreen?'shrink':'expand'} className="quiet" onClick={onFull}>{fullscreen?'退出全屏':'全屏'}</RemoteTool><i className="remote-flex"/>
      <span className="remote-control-state"><i className="remote-status-dot"/>{session.paused?'共享已暂停':control?'控制已授权':'仅查看'}</span><RemoteTool icon="control" className="quiet" disabled={!active||busy||session.paused||(!control&&(!session.native_host||session.control_requested))} onClick={()=>onAction({type:control?'release_control':'request_control'})}>{control?'释放控制':session.control_requested?'等待授权':'请求控制'}</RemoteTool><i className="remote-divider"/><RemoteTool icon="chat" className="quiet" onClick={onChat}>回到聊天</RemoteTool>
      <span ref={infoButton}><RemoteTool icon="info" className={info?'selected':'quiet'} aria-expanded={info} onClick={()=>setInfo(value=>!value)}>主机信息</RemoteTool></span>{info&&<div ref={popover}><ConnectionInfo remote={remote} onClose={()=>{setInfo(false);infoButton.current?.querySelector('button')?.focus();}}/></div>}
    </div>
    {(remote.error||remote.inputError)&&<p className="remote-error" role="alert">{remote.error||remote.inputError}</p>}
    {active&&remote.audioBlocked&&<div className="remote-viewer-audio-notice" role="status"><RemoteIcon name="sound"/><span>点击开启本窗口的通话声音</span><RemoteTool className="quiet" disabled={busy} onClick={onPlayAudio}>开启声音</RemoteTool></div>}
    <main className="remote-viewer-canvas">{active?children:<div className="remote-viewer-ended"><RemoteIcon size={40}/><h2>远程协助已结束</h2><p>屏幕和控制连接已关闭，聊天记录保留在主窗口中。</p><RemoteTool icon="chat" onClick={onChat}>回到聊天</RemoteTool></div>}</main>
    <footer className="remote-viewer-metrics"><span><i className={`remote-status-dot ${active?'':'offline'}`}/>{active?connectionLabel(remote):'已结束'}</span><button onClick={()=>setInfo(value=>!value)} title="查看连接性能"><RemoteIcon name="signal"/>{number(m.rtt,' ms')}</button><span>{number(m.fps,' FPS')}</span>{m.width&&m.height?<span>{m.width} × {m.height}</span>:null}<small>{session.paused?'已暂停':quality} · {scale==='actual'?'100%':'适应窗口'}</small></footer>
  </>;
}
