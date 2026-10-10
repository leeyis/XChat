import {useEffect,useRef,useState} from 'react';
import {RemoteFloatingPanel,SharingControls} from './RemoteControls.jsx';
import {remoteLive} from './remote-model.js';
import {clearPanelCollapsed,newPanelMemory} from './remote-panel.js';
import {createNativePanelAdapter} from './remote-native-panel.js';

export default function RemoteToolbar() {
  const [session,setSession]=useState(null),[error,setError]=useState(''),[busy,setBusy]=useState(false),[adapter,setAdapter]=useState(null);
  const lock=useRef(false),memory=useRef(newPanelMemory());
  const params=new URLSearchParams(location.search),actor=params.get('actor'),id=params.get('session');
  const call=action=>window.__TAURI__.core.invoke('remote_toolbar',{actor,id,action:action||null});
  const receive=next=>setSession(previous=>!previous||!next||next.version>=previous.version?next:previous);
  useEffect(()=>{
    const scheme=matchMedia('(prefers-color-scheme: dark)');
    const theme=()=>{let value='system';try{value=localStorage.getItem('xchat.theme')||'system';}catch{}document.documentElement.dataset.theme=value==='system'?(scheme.matches?'dark':'light'):value;};
    document.documentElement.classList.add('remote-toolbar-window');theme();scheme.addEventListener('change',theme);window.addEventListener('storage',theme);
    const native=createNativePanelAdapter(window.__TAURI__.window,setError);setAdapter(native);
    return()=>{native.dispose();document.documentElement.classList.remove('remote-toolbar-window');scheme.removeEventListener('change',theme);window.removeEventListener('storage',theme);};
  },[]);
  useEffect(()=>{
    let alive=true,pending=false;
    const poll=async()=>{if(pending)return;pending=true;try{
      const result=await call();if(!alive)return;
      if(!remoteLive(result.session)){clearPanelCollapsed(id);await window.__TAURI__.window.getCurrentWindow().close();return;}
      receive(result.session);
    }catch(e){if(alive)setError(String(e));}finally{pending=false;}};
    void poll();const timer=setInterval(poll,600);return()=>{alive=false;clearInterval(timer);};
  },[actor,id]);
  const act=async action=>{
    const urgent=action.type==='stop'||(action.type==='control'&&!action.allow);
    if(lock.current&&!urgent)return;
    lock.current=true;setBusy(true);setError('');
    try{receive((await call(action)).session);}catch(e){setError(String(e));}finally{lock.current=false;setBusy(false);}
  };
  const revoke=()=>act({type:'control',allow:false}),end=()=>act({type:'stop',reason:'ended'});
  return <div className="remote-native-shell">
    {session&&adapter&&<RemoteFloatingPanel key={session.id} session={session} memory={memory.current} nativeAdapter={adapter} busy={busy} onRevoke={revoke} onEnd={end}>
      {props=><SharingControls {...props} session={session} busy={busy} onRevoke={revoke} onPause={()=>act({type:'pause',paused:!session.paused})} onEnd={end}/>}
    </RemoteFloatingPanel>}
    {error&&<div className="remote-native-error" role="alert">{error}</div>}
  </div>;
}
