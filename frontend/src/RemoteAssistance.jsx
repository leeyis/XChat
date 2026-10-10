import {useEffect,useLayoutEffect,useRef,useState,useSyncExternalStore} from "react";
import {createPortal} from "react-dom";
import StabilityDialog from "./StabilityDialog.jsx";
import {remoteClient} from "./remote-client.js";
import {remoteAccepted,remoteLive,remoteDuration} from "./remote-model.js";
import "./remote.css";
import {RemoteIcon,RemoteTool,RemoteFloatingPanel,SharingControls} from "./RemoteControls.jsx";
import {RemoteVoiceBar} from "./RemoteSessionView.jsx";
import {newPanelMemory,clearPanelCollapsed} from "./remote-panel.js";
export {RemoteIcon} from "./RemoteControls.jsx";

export const openRemote=peerId=>window.dispatchEvent(new CustomEvent("xchat:remote-open",{detail:{peerId}}));
const failureText={rejected:["对方暂时没有同意","本次请求已结束，没有共享屏幕或授予控制。"],cancelled:["请求已取消","可以在对方方便时重新发起。"],expired:["对方暂未回应","请求已过期，请先在聊天中确认对方是否方便。"],ended:["远程协助已结束","屏幕与语音已停止，鼠标键盘权限已收回。"],disconnected:["远程连接已中断","共享已停止。重新连接需要电脑主人再次同意。"],locked:["共享的电脑已锁屏","屏幕、控制和语音均已结束，解锁后可重新发起。"]};

export default function RemoteAssistance({state,workspace}){
  const remote=useSyncExternalStore(remoteClient.subscribe,remoteClient.snapshot),session=remote.session;
  const [opened,setOpened]=useState(false),[peerId,setPeerId]=useState(null),[dialog,setDialog]=useState(null),[mode,setMode]=useState("help");
  const [screens,setScreens]=useState([]),[screen,setScreen]=useState(null),[voice,setVoice]=useState(true),[control,setControl]=useState(false),[note,setNote]=useState("");
  const [busy,setBusy]=useState(false),[error,setError]=useState("");
  const [audioDevices,setAudioDevices]=useState([]),[,setTick]=useState(0);
  const seen=useRef(null),lock=useRef(false),surface=useRef(null),focusPrior=useRef(null),offerRevision=useRef(null);
  const panels=useRef({id:null,memory:newPanelMemory()});
  if(panels.current.id!==session?.id)panels.current={id:session?.id,memory:newPanelMemory()};
  const [anchors,setAnchors]=useState({top:null,voice:null}),[audioInput,setAudioInput]=useState(""),[audioOutput,setAudioOutput]=useState("");
  const live=remoteLive(session),active=remoteAccepted(session),selectedPeer=state.devices.find(p=>p.id===(live?session.peer_id:peerId));
  const name=live?session.peer_name:(selectedPeer?.remark||selectedPeer?.name||session?.peer_name||"对方");
  const conversation=state.conversations.find(c=>c.kind==="direct"&&c.peer_id===(live?session.peer_id:peerId));
  const run=async fn=>{if(lock.current)return;lock.current=true;setBusy(true);setError("");try{return await fn();}catch(e){if(e.name!=="NotAllowedError"||!String(e.message).includes("cancel"))setError(String(e.message||e));}finally{lock.current=false;setBusy(false);}};
  const sources=async()=>{const found=await remoteClient.screens();setScreens(found);setScreen(found[0]||null);};
  useEffect(()=>{
    const open=event=>{
      const current=remoteClient.snapshot().session;
      if(remoteAccepted(current)&&current.peer_id===event.detail.peerId){
        window.dispatchEvent(new CustomEvent("xchat:remote-chat",{detail:{peerId:current.peer_id}}));
        if(!current.local_host)void remoteClient.openViewer().catch(e=>setError(String(e.message||e)));
        return;
      }
      focusPrior.current=document.activeElement;setPeerId(event.detail.peerId);setOpened(true);setError("");
    };
    window.addEventListener("xchat:remote-open",open);return()=>window.removeEventListener("xchat:remote-open",open);
  },[]);
  useEffect(()=>{const timer=setInterval(()=>setTick(n=>n+1),1000);return()=>clearInterval(timer);},[]);
  useEffect(()=>{
    if(session?.id!==seen.current){clearPanelCollapsed(seen.current);seen.current=session?.id;
      if(session&&!session.initiator&&session.phase==="waiting"){
        focusPrior.current=document.activeElement;setPeerId(session.peer_id);setOpened(true);setVoice(session.offered_voice);setDialog("consent");setControl(false);
        if(session.local_host)void sources().catch(e=>setError(String(e.message||e)));
      }
    }
    if(session&&!remoteLive(session)){setDialog(null);clearPanelCollapsed(session.id);}
  },[session?.id,session?.phase]);
  useEffect(()=>{if(session?.local_host&&session.control_requested&&session.phase==="active"){setOpened(true);setDialog("control");}},[session?.id,session?.control_requested]);
  useEffect(()=>{if(active&&conversation){setOpened(false);window.dispatchEvent(new CustomEvent("xchat:remote-chat",{detail:{peerId:session.peer_id}}));}},[active,session?.id,conversation?.id]);
  useLayoutEffect(()=>{
    const find=slot=>active?[...document.querySelectorAll('[data-remote-slot="'+slot+'"]')].find(node=>node.dataset.remotePeer===session.peer_id)||null:null;
    const top=find("session"),voiceSlot=find("voice");
    setAnchors(current=>current.top===top&&current.voice===voiceSlot?current:{top,voice:voiceSlot});
  },[active,session?.peer_id,conversation?.id,state.activeConversationId,state.activeSection]);
  const closeDialog=()=>{if(busy)return;if(dialog==="control"){void run(async()=>{await remoteClient.act({type:"control",allow:false});setDialog(null);});return;}setDialog(null);if(!live)remoteClient.discardSource();};
  const minimize=()=>{setOpened(false);focusPrior.current?.focus?.();};
  useEffect(()=>{
    if(!opened||dialog)return;
    const navigate=event=>{if(event.target.closest?.('.rail button'))setOpened(false);};
    const escape=event=>{if(event.key==='Escape'&&!document.fullscreenElement){setOpened(false);focusPrior.current?.focus?.();}};
    document.addEventListener('click',navigate);document.addEventListener('keydown',escape);
    return()=>{document.removeEventListener('click',navigate);document.removeEventListener('keydown',escape);};
  },[opened,dialog]);
  const prepare=async next=>{setMode(next);setNote("");setVoice(true);setControl(false);setDialog("request");if(next==="help")await sources();};
  const chooseBrowser=()=>run(async()=>{const selected=await remoteClient.browserScreen();remoteClient.setSource(selected.stream);setScreen(selected.screen);});
  const screenChoices=()=>remote.nativeHost?<div className="remote-screen-options">{screens.map(item=><label key={item.id} className={screen?.id===item.id?"selected":""}><input type="radio" name="remote-screen" checked={screen?.id===item.id} onChange={()=>setScreen(item)}/><span className="remote-screen-symbol"><RemoteIcon size={34}/></span><b>{item.name}</b><small>{item.width} × {item.height}</small></label>)}{!screens.length&&<p>没有可共享的显示器。</p>}</div>:<div className="remote-browser-source"><RemoteIcon/><div><b>{screen?.name||"选择要共享的屏幕或窗口"}</b><small>{screen?`${screen.width} × ${screen.height}`:"由系统选择器确认，此共享方式仅允许查看。"}</small></div><button disabled={busy} onClick={chooseBrowser}>{screen?"重新选择":"选择屏幕"}</button></div>;
  const voiceChoice=()=> <label className="remote-choice"><input type="checkbox" checked={voice} onChange={e=>setVoice(e.target.checked)}/><span><b><RemoteIcon name="phone" size={16}/>{dialog==="consent"?"同时接通双向语音":"同时发起语音通话"}</b><small>双方同意后边说边协作，可随时静音或单独挂断。</small></span></label>;
  const accept=permission=>run(async()=>{await remoteClient.act({type:"accept",screen:session.local_host?screen:null,control:permission,voice});setDialog(null);});
  const act=action=>run(()=>remoteClient.act(action));
  const clearHome=()=>{setPeerId(session?.peer_id||peerId);remoteClient.dismiss();};
  const end=()=>remoteClient.act({type:"stop",reason:session?.phase==="waiting"?"cancelled":"ended"}).catch(e=>setError(String(e.message||e)));
  const stateMessage=session?.phase==="waiting"?(session.initiator?[session.mode==="help"?`正在邀请 ${name} 协助我`:`正在请求 ${name} 的远程协助`,"对方同意前，不会发送画面或开放控制权限。"]:[session.mode==="help"?`${name} 请求你远程协助`:`${name} 请求控制你的电脑`,"由共享方选择屏幕，并明确决定本次权限。"]):failureText[session?.phase]||["对方已同意，正在建立连接","聊天仍可继续使用。"];

  const focusConversation=()=>window.dispatchEvent(new CustomEvent("xchat:remote-chat",{detail:{peerId:session?.peer_id}}));
  const openViewer=()=>run(()=>remoteClient.openViewer());
  const openAudio=()=>run(async()=>{if(!navigator.mediaDevices?.enumerateDevices)throw new Error("当前环境无法列出音频设备");setAudioDevices(await navigator.mediaDevices.enumerateDevices());setDialog("audio");});
  const voiceAction=action=>{
    if(["speaker","play_audio"].includes(action.type))void run(()=>remoteClient.mediaAction(action));
    else {if(action.type==="voice_invite")remoteClient.publish({voiceError:""});void act(action);}
  };
  const chooseControl=()=>{offerRevision.current=session.revision;setDialog(session.control_requested?"control":"control-offer");};
  const chooseScreen=()=>run(async()=>{if(remote.nativeHost){await sources();setDialog("screen");}else await remoteClient.switchBrowserScreen();});
  const sessionBanner=active&&<div className="remote-session">
    <section className="remote-chat-session-bar" aria-label="远程协助状态">
      <span className="remote-chat-session-emblem"><RemoteIcon name={session.paused?"pause":"screen"}/></span>
      <div className="remote-chat-session-copy"><strong>{session.local_host?(session.paused?"屏幕共享已暂停":"正在向 "+name+" 共享屏幕"):(session.phase==="active"?"远程桌面已在独立窗口中连接":"正在建立远程协助连接")}</strong>
        <p><i className="remote-status-dot"/>{session.local_host?(session.paused?"对方暂时看不到新的画面":session.grant?"对方可控制鼠标和键盘":"对方仅可查看屏幕"):(remote.connectionState==="connected"?"远程连接已接通":"正在建立远程连接")} · {session.local_host?session.screen?.name:remoteDuration(session.started_at)}</p>
      </div>
      <div className="remote-chat-session-actions">
        {!session.local_host&&<RemoteTool icon="external" className="primary" disabled={busy} onClick={openViewer}>打开远程窗口</RemoteTool>}
        {session.local_host&&<>{session.grant?<RemoteTool icon="shield" className="danger" disabled={busy} onClick={()=>act({type:"control",allow:false})}>收回控制</RemoteTool>:session.native_host&&!session.paused&&<RemoteTool icon="shield" disabled={busy} onClick={chooseControl}>{session.control_requested?"处理控制请求":"允许控制"}</RemoteTool>}
          <RemoteTool icon={session.paused?"play":"pause"} className="quiet" disabled={busy||session.phase!=="active"} onClick={()=>act({type:"pause",paused:!session.paused})}>{session.paused?"继续共享":"暂停共享"}</RemoteTool>
          <RemoteTool icon="screen" className="quiet icon-only" title="切换共享屏幕" aria-label="切换共享屏幕" disabled={busy} onClick={chooseScreen}/>
        </>}
        {!["active","ringing"].includes(session.voice?.stage)&&<RemoteTool icon="phone" className="quiet icon-only" title="发起语音通话" aria-label="发起语音通话" disabled={busy} onClick={()=>voiceAction({type:"voice_invite"})}/>}
        <RemoteTool icon="close" className="danger quiet icon-only" title="结束协助" aria-label="结束协助" onClick={end}/>
      </div>
    </section>
    {(error||remote.error||remote.inputError)&&<p className="remote-chat-notice" role="alert">{error||remote.error||remote.inputError}</p>}
  </div>;
  return <>
    {active&&anchors.top&&createPortal(sessionBanner,anchors.top)}
    {active&&anchors.voice&&createPortal(<RemoteVoiceBar remote={remote} name={name} busy={busy} onAction={voiceAction} onAudio={openAudio}/>,anchors.voice)}
    {active&&!anchors.top&&<button className="remote-floating" onClick={focusConversation}><RemoteIcon/><span>{session.local_host?"我的屏幕正在共享":"远程协助进行中"}<small>{name}</small></span><b>回到聊天</b></button>}
    {active&&session.local_host&&!remote.nativeHost&&createPortal(<div className="remote-browser-sharing remote-session"><RemoteFloatingPanel key={session.id} session={session} memory={panels.current.memory} busy={busy} onRevoke={()=>act({type:"control",allow:false})} onEnd={end}>{props=><SharingControls {...props} session={session} busy={busy} onRevoke={()=>act({type:"control",allow:false})} onPause={()=>act({type:"pause",paused:!session.paused})} onEnd={end}/>}</RemoteFloatingPanel></div>,document.body)}
    {!opened&&live&&!active&&<button className="remote-floating" onClick={()=>setOpened(true)}><RemoteIcon/><span>{session.phase==="waiting"?"远程协助请求":session.paused?"画面已暂停":"远程协助进行中"}<small>{name}</small></span><b>打开</b></button>}
    {opened&&!active&&createPortal(<section ref={surface} className={`remote-workspace ${session?.phase==="active"?"remote-session":""}`} aria-label="远程协助工作区">
      {session?.phase!=="active"&&<header className="remote-head"><span className="remote-avatar">{name.slice(0,1)}</span><div><b>{active?(session.local_host?"正在共享我的屏幕":`${name} 的远程桌面`):"远程协助"}</b><small>{active?`${session.local_host?"共享方":"协助方"} · ${remoteDuration(session.started_at)}`:`与 ${name} 一起，解决电脑上的问题`}</small></div><div className="remote-head-actions">{live&&<button className="remote-danger" disabled={busy} onClick={end}>× {session.phase==="waiting"?"取消请求":"结束协助"}</button>}<button onClick={minimize}>返回聊天</button></div></header>}
      {(error||remote.error||remote.inputError)&&<p className="remote-error" role="alert">{error||remote.error||remote.inputError}</p>}
      {session?<div className="remote-lobby"><section className="remote-request-card"><span className="remote-request-icon"><RemoteIcon name={session.mode==="help"?"help":"control"} size={32}/></span><small>{session.mode==="help"?"请求对方协助":"请求控制对方"}</small><h2>{stateMessage[0]}</h2><p>{stateMessage[1]}</p><div className="remote-route"><span>{session.local_host?name:"我"}<small>协助 / 操作方</small></span><b>→</b><span>{session.local_host?"我的电脑":`${name} 的电脑`}<small>屏幕共享方</small></span></div>{session.note&&<blockquote>{session.note}</blockquote>}<div className="remote-request-actions">{session.phase==="waiting"&&!session.initiator?<><button disabled={busy} onClick={()=>act({type:"stop",reason:"rejected"})}>拒绝</button><button className="primary-button" onClick={()=>setDialog("consent")}>查看请求</button></>:live?<button disabled={busy} onClick={end}>{session.phase==="waiting"?"取消请求":"取消连接"}</button>:<><button onClick={clearHome}>返回选择</button><button className="primary-button" onClick={()=>run(async()=>{const next=session.mode;clearHome();await prepare(next);})}>{session.mode==="help"?"重新邀请对方协助":"重新请求控制对方"}</button></>}</div></section></div>:<div className="remote-lobby"><div className="remote-intro"><small>从当前会话开始</small><h1>哪一台电脑需要帮助？</h1><p>选择需要帮助的电脑，接通后可双向语音协作。</p></div>{(!remote.available||selectedPeer?.is_offline)&&<p className="remote-unavailable">{selectedPeer?.is_offline?"对方暂时离线，上线后可发起协助。":remote.error||"正在检查远程协助能力…"}</p>}<div className="remote-mode-grid">{[["help","请求对方协助","共享我的屏幕，邀请对方一起解决问题。","对方协助我"],["control","请求控制对方","由对方选择共享屏幕，并明确允许本次操作。","我协助对方"]].map(([value,title,copy,tag])=><button key={value} className="remote-mode-card" disabled={!remote.available||busy||!selectedPeer||selectedPeer.is_offline} onClick={()=>run(()=>prepare(value))}><RemoteIcon name={value==="help"?"help":"control"} size={32}/><small>{tag}</small><h2>{title}</h2><p>{copy}</p><span>发起请求　→</span></button>)}</div><p className="remote-lobby-note"><RemoteIcon name="shield"/>共享哪块屏幕、是否允许操作，都由电脑主人决定。可随时暂停或结束。</p></div>}
    </section>,document.body)}
    {dialog==="request"&&<StabilityDialog portalRoot={document.fullscreenElement||document.body} title={mode==="help"?`请求 ${name} 远程协助`:`请求控制 ${name} 的电脑`} onClose={busy?undefined:closeDialog} actions={<><button disabled={busy} onClick={closeDialog}>取消</button><button className="primary-button" disabled={busy||(mode==="help"&&!screen)} onClick={()=>run(async()=>{await remoteClient.start(peerId,{mode,note,screen:mode==="help"?screen:null,control:mode==="help"&&control,voice});setDialog(null);})}>{mode==="help"?"发送协助邀请":"发送控制请求"}</button></>}><p>{mode==="help"?"选择要共享的屏幕。对方接受后，才会开始传送画面。":"对方需要选择共享屏幕，并决定允许控制、仅查看或拒绝。"}</p>{mode==="help"&&<>{screenChoices()}<label className="remote-choice"><input type="checkbox" checked={control} disabled={!remote.nativeHost} onChange={e=>setControl(e.target.checked)}/><span><b>同时允许对方操作我的鼠标和键盘</b><small>{remote.nativeHost?"仅本次会话；不勾选时只共享画面。":"当前浏览器共享只支持查看。"}</small></span></label></>}{voiceChoice()}<label className="remote-note">协助说明 <small>选填</small><textarea maxLength={200} rows={2} value={note} onChange={e=>setNote(e.target.value)} placeholder="例如：帮我检查一下网络设置"/></label>{error&&<p className="stability-error" role="alert">{error}</p>}</StabilityDialog>}
    {dialog==="consent"&&session?.phase==="waiting"&&<StabilityDialog portalRoot={document.fullscreenElement||document.body} title={session.local_host?`${name} 请求控制你的电脑`:`${name} 请求你远程协助`} onClose={busy?undefined:closeDialog} actions={<><button disabled={busy} onClick={()=>run(async()=>{await remoteClient.act({type:"stop",reason:"rejected"});setDialog(null);})}>拒绝</button>{session.local_host?<><button disabled={busy||!screen} onClick={()=>accept(false)}>仅允许查看</button><button className="primary-button" disabled={busy||!screen||!remote.nativeHost} onClick={()=>accept(true)}>允许本次控制</button></>:<button className="primary-button" disabled={busy} onClick={()=>accept(session.offered_control)}>接受协助</button>}</>}>{session.note&&<blockquote>{session.note}</blockquote>}<p>{session.local_host?"先选择共享的屏幕，再决定本次允许的权限。":`接受后可查看对方的 ${session.screen?.name||"屏幕"}。${session.offered_control?"对方已明确允许本次鼠标键盘控制。":"目前仅允许查看，之后可以单独申请控制。"}`}</p>{session.local_host&&<>{screenChoices()}<p>可随时收回控制。此次授权不包含剪贴板同步或文件访问接口。</p></>}{session.offered_voice&&voiceChoice()}{error&&<p className="stability-error" role="alert">{error}</p>}</StabilityDialog>}
    {dialog==="control"&&session?.control_requested&&<StabilityDialog portalRoot={document.fullscreenElement||document.body} title={`允许 ${name} 控制你的电脑？`} onClose={busy?undefined:closeDialog} actions={<><button disabled={busy} onClick={()=>run(async()=>{await remoteClient.act({type:"control",allow:false});setDialog(null);})}>保持仅查看</button><button className="primary-button" disabled={busy} onClick={()=>run(async()=>{await remoteClient.act({type:"control",allow:true});setDialog(null);})}>允许本次控制</button></>}><p>{session.screen?.name} · 本次鼠标与键盘操作</p><p>暂停、切屏、断线或锁屏后，控制权限自动失效。你可随时在共享提示条中收回控制。</p></StabilityDialog>}
    {dialog==="control-offer"&&session?.local_host&&session?.phase==="active"&&<StabilityDialog portalRoot={document.fullscreenElement||document.body} title={`允许 ${name} 控制你的电脑？`} onClose={busy?undefined:closeDialog} actions={<><button disabled={busy} onClick={closeDialog}>保持仅查看</button><button className="primary-button" disabled={busy||session.paused} onClick={()=>run(async()=>{await remoteClient.act({type:"offer_control",revision:offerRevision.current});setDialog(null);})}>允许本次控制</button></>}><p>{session.screen?.name} · 本次鼠标与键盘操作</p><p>暂停、切屏、断线或锁屏后，控制权限自动失效。你可随时在共享提示条中收回控制。</p>{error&&<p className="stability-error" role="alert">{error}</p>}</StabilityDialog>}
    {dialog==="screen"&&<StabilityDialog portalRoot={document.fullscreenElement||document.body} title="切换共享屏幕" onClose={busy?undefined:closeDialog} actions={<><button disabled={busy} onClick={closeDialog}>取消</button><button className="primary-button" disabled={busy||!screen} onClick={()=>run(async()=>{await remoteClient.act({type:"screen",screen});setDialog(null);})}>共享所选屏幕</button></>}><p>切换后收回控制，继续保持仅查看。语音通话保持连接。</p>{screenChoices()}</StabilityDialog>}
    {dialog==="audio"&&<StabilityDialog portalRoot={document.body} title="音频设置" onClose={busy?undefined:closeDialog} actions={<button onClick={closeDialog}>完成</button>}><label className="remote-note">麦克风<select value={audioInput} onChange={e=>{const deviceId=e.target.value;void run(async()=>{await remoteClient.mediaAction({type:"microphone",deviceId});setAudioInput(deviceId);});}}><option value="">系统默认</option>{audioDevices.filter(d=>d.kind==="audioinput").map((device,i)=><option key={device.deviceId} value={device.deviceId}>{device.label||"麦克风 "+(i+1)}</option>)}</select></label><label className="remote-note">扬声器<select disabled={!globalThis.HTMLMediaElement?.prototype?.setSinkId} value={audioOutput} onChange={e=>{const deviceId=e.target.value;void run(async()=>{await remoteClient.mediaAction({type:"output",deviceId});setAudioOutput(deviceId);});}}><option value="">系统默认</option>{audioDevices.filter(d=>d.kind==="audiooutput").map((device,i)=><option key={device.deviceId} value={device.deviceId}>{device.label||"扬声器 "+(i+1)}</option>)}</select></label><p>通话使用麦克风，不共享电脑的系统声音。</p>{error&&<p className="stability-error" role="alert">{error}</p>}</StabilityDialog>}
  </>;
}
