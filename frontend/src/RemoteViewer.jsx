import {useEffect,useRef,useState,useSyncExternalStore} from 'react';
import {RemoteClient} from './remote-client.js';
import {RemoteIcon} from './RemoteControls.jsx';
import RemoteSessionView from './RemoteSessionView.jsx';
import RemoteVideo from './RemoteVideo.jsx';
import StabilityDialog from './StabilityDialog.jsx';
import './remote.css';
import './remote-viewer.css';

export default function RemoteViewer({client:providedClient}) {
  const [client]=useState(()=>{const params=new URLSearchParams(location.search);return providedClient||new RemoteClient({viewer:{actor:params.get('actor'),id:params.get('session')}});});
  const remote=useSyncExternalStore(client.subscribe,client.snapshot),session=remote.session;
  const [scale,setScale]=useState('fit'),[fullscreen,setFullscreen]=useState(false),[quality,setQuality]=useState(null),[busy,setBusy]=useState(false),[error,setError]=useState('');
  const surface=useRef(null),locked=useRef(false);
  useEffect(()=>{
    document.documentElement.classList.add('remote-viewer-window');
    const scheme=matchMedia('(prefers-color-scheme: dark)');
    const theme=()=>{let value='system';try{value=localStorage.getItem('xchat.theme')||'system';}catch{}document.documentElement.dataset.theme=value==='system'?(scheme.matches?'dark':'light'):value;};
    let disposed=false;
    const nativeWindow=window.__TAURI__?.window?.getCurrentWindow();
    const syncFull=()=>{if(nativeWindow)void nativeWindow.isFullscreen().then(value=>{if(!disposed)setFullscreen(value);}).catch(()=>{});else setFullscreen(Boolean(document.fullscreenElement));};
    theme();syncFull();scheme.addEventListener('change',theme);window.addEventListener('storage',theme);window.addEventListener('resize',syncFull);window.addEventListener('focus',syncFull);document.addEventListener('fullscreenchange',syncFull);
    return()=>{disposed=true;document.documentElement.classList.remove('remote-viewer-window');scheme.removeEventListener('change',theme);window.removeEventListener('storage',theme);window.removeEventListener('resize',syncFull);window.removeEventListener('focus',syncFull);document.removeEventListener('fullscreenchange',syncFull);};
  },[]);
  useEffect(()=>{document.title=session?.peer_name?`${session.peer_name} · XChat 远程桌面`:'XChat · 远程桌面';},[session?.peer_name]);
  const run=async callback=>{if(locked.current)return;locked.current=true;setBusy(true);setError('');try{return await callback();}catch(cause){setError(String(cause.message||cause));}finally{locked.current=false;setBusy(false);}};
  const action=value=>run(async()=>{if(['release_control','stop'].includes(value.type))client.media?.sendInput({type:'release'});return client.act(value);});
  const full=()=>run(async()=>{client.media?.sendInput({type:'release'});if(window.__TAURI__?.window){const win=window.__TAURI__.window.getCurrentWindow(),next=!(await win.isFullscreen());await win.setFullscreen(next);setFullscreen(next);}else if(document.fullscreenElement)await document.exitFullscreen();else await surface.current?.requestFullscreen();});
  const chat=()=>run(()=>{client.media?.sendInput({type:'release'});return client.focusChat();});
  const escape=event=>{if(event.key==='Escape'&&fullscreen&&!quality&&window.__TAURI__?.window){event.preventDefault();void run(async()=>{client.media?.sendInput({type:'release'});await window.__TAURI__.window.getCurrentWindow().setFullscreen(false);setFullscreen(false);});}};
  const end=()=>{client.media?.sendInput({type:'release'});void client.act({type:'stop',reason:'ended'}).catch(cause=>setError(String(cause.message||cause)));};
  return <section ref={surface} onKeyDown={escape} className={`remote-viewer remote-session ${fullscreen?'is-fullscreen':''}`} aria-label="独立远程桌面窗口">
    {error&&<p className="remote-error" role="alert">{error}</p>}
    {session&&!session.local_host?<RemoteSessionView remote={remote} name={session.peer_name} busy={busy} scale={scale} setScale={setScale} onEnd={end} onAction={action} onQuality={()=>setQuality({...session.quality})} onChat={chat} onPlayAudio={()=>run(()=>client.mediaAction({type:'play_audio'}))} onFull={full} fullscreen={fullscreen}><RemoteVideo remote={remote} client={client} scale={scale}/></RemoteSessionView>:<div className="remote-viewer-loading"><RemoteIcon size={40}/><h1>{remote.ready&&remote.error?'无法打开远程桌面':'正在连接远程桌面…'}</h1><p>{remote.error||'聊天仍保留在 XChat 主窗口中。'}</p></div>}
    {quality&&<StabilityDialog portalRoot={document.fullscreenElement||document.body} title="画面质量" onClose={busy?undefined:()=>setQuality(null)} actions={<><button disabled={busy} onClick={()=>setQuality(null)}>取消</button><button className="primary-button" disabled={busy} onClick={()=>void run(async()=>{await client.act({type:'quality',quality});setQuality(null);})}>应用</button></>}>
      {[["auto","自动画质","根据连接情况调整编码码率，优先保证操作响应"],["fluent","流畅优先","减少画面细节，适合网络不稳定时排查问题"],["clear","清晰优先","保留更多文字细节，适合查看文档"]].map(([value,title,copy])=><label className="remote-choice" key={value}><input type="radio" name="viewer-quality" checked={quality.preset===value} onChange={()=>setQuality({...quality,preset:value})}/><span><b>{title}</b><small>{copy}</small></span></label>)}
      <label className="remote-note">画面刷新上限<select value={quality.fps} onChange={event=>setQuality({...quality,fps:Number(event.target.value)})}>{[10,20,30,60].map(fps=><option key={fps} value={fps}>{fps} 帧 / 秒</option>)}</select></label><label className="remote-choice"><input type="checkbox" checked={quality.reduced_color} disabled={!session?.native_host} onChange={event=>setQuality({...quality,reduced_color:event.target.checked})}/>减少色彩，降低带宽占用</label>{error&&<p className="stability-error" role="alert">{error}</p>}
    </StabilityDialog>}
  </section>;
}
