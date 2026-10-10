import {useEffect,useState} from "react";
import {RemoteIcon} from "./RemoteAssistance.jsx";
import {remoteLive} from "./remote-model.js";

export default function RemoteToolbar(){
  const [session,setSession]=useState(null),[error,setError]=useState(""),[busy,setBusy]=useState(false);
  const params=new URLSearchParams(location.search),actor=params.get("actor"),id=params.get("session");
  const call=action=>window.__TAURI__.core.invoke("remote_toolbar",{actor,id,action:action||null});
  useEffect(()=>{
    const scheme=matchMedia("(prefers-color-scheme: dark)");
    const theme=()=>{let value="system";try{value=localStorage.getItem("xchat.theme")||"system";}catch{}document.documentElement.dataset.theme=value==="system"?(scheme.matches?"dark":"light"):value;};
    theme();scheme.addEventListener("change",theme);window.addEventListener("storage",theme);
    return()=>{scheme.removeEventListener("change",theme);window.removeEventListener("storage",theme);};
  },[]);
  useEffect(()=>{
    let alive=true,pending=false;const poll=async()=>{if(pending)return;pending=true;try{
      const result=await call();if(!alive)return;
      if(!remoteLive(result.session)){await window.__TAURI__.window.getCurrentWindow().close();return;}
      setSession(result.session);
    }catch(e){if(alive)setError(String(e));}finally{pending=false;}};
    void poll();const timer=setInterval(poll,600);return()=>{alive=false;clearInterval(timer);};
  },[actor,id]);
  const act=async action=>{if(busy)return;setBusy(true);setError("");try{const result=await call(action);setSession(result.session);}catch(e){setError(String(e));}finally{setBusy(false);}};
  return <div className="remote-host-toolbar"><RemoteIcon/><div data-tauri-drag-region><b data-tauri-drag-region>{session?.phase==="waiting"?"等待对方接受":session?.paused?"画面已暂停":"正在共享屏幕"}</b><small data-tauri-drag-region>{error||`${session?.peer_name||"远程协助"} · ${session?.grant?"对方可操作":"仅查看"}`}</small></div><button disabled={busy||!session?.grant} onClick={()=>act({type:"control",allow:false})}>收回控制</button><button disabled={busy||!session||session.phase==="waiting"} onClick={()=>act({type:"pause",paused:!session.paused})}>{session?.paused?"继续共享":"暂停画面"}</button><button disabled={busy||session?.voice.stage!=="active"} onClick={()=>act({type:"muted",muted:!session.voice.local_muted})}><RemoteIcon name="mic" size={15}/>{session?.voice.local_muted?"开麦":"静音"}</button><button className="remote-danger" disabled={busy} onClick={()=>act({type:"stop",reason:"ended"})}>结束协助</button></div>;
}
