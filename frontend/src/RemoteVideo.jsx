import {useEffect,useLayoutEffect,useRef} from 'react';
import {remoteInputAllowed,remotePoint} from './remote-model.js';
import {RemoteIcon} from './RemoteControls.jsx';

const dimensions=element=>[element.videoWidth||element.width,element.videoHeight||element.height];

// The receiver presents HEVC directly; RTP keeps its video element as a fallback.
export default function RemoteVideo({remote,client,scale}) {
  const surface=useRef(null),lastPoint=useRef(null),move=useRef(null),frame=useRef(0);
  const session=remote.session,allowed=remoteInputAllowed(session,remote.owned),stream=remote.remoteScreen;
  const hevc=client.media?.hevc,directCanvas=!!hevc?.receiving,Surface=directCanvas?'canvas':'video';
  useLayoutEffect(()=>{
    if(directCanvas&&surface.current)return hevc.attachSurface(surface.current);
  },[hevc,directCanvas,session?.id]);
  useEffect(()=>{if(directCanvas)return;const element=surface.current;if(element){element.srcObject=stream;void element.play().catch(()=>{});}return()=>{if(element)element.srcObject=null;};},[stream,directCanvas,session?.id]);
  useEffect(()=>{const release=()=>client.media?.sendInput({type:'release'});window.addEventListener('blur',release);document.addEventListener('visibilitychange',release);return()=>{release();window.removeEventListener('blur',release);document.removeEventListener('visibilitychange',release);cancelAnimationFrame(frame.current);frame.current=0;move.current=null;lastPoint.current=null;};},[client,session?.id,directCanvas]);
  useEffect(()=>{
    const element=surface.current;if(!element||!allowed)return;
    const wheel=event=>{const at=remotePoint(element.getBoundingClientRect(),...dimensions(element),event.clientX,event.clientY);if(!at)return;event.preventDefault();const factor=event.deltaMode===1?16:event.deltaMode===2?element.clientHeight:1;client.media?.sendInput({type:'wheel',...at,delta:Math.max(-1200,Math.min(1200,Math.round(-(event.deltaY||event.deltaX)*factor))),horizontal:!event.deltaY});};
    element.addEventListener('wheel',wheel,{passive:false});return()=>element.removeEventListener('wheel',wheel);
  },[allowed,client,session?.id,directCanvas]);
  if(!session||session.local_host)return null;
  const point=event=>{const element=surface.current;return element?remotePoint(element.getBoundingClientRect(),...dimensions(element),event.clientX,event.clientY):null;};
  const pointer=(event,down)=>{if(!allowed)return;const at=point(event)||(!down?lastPoint.current:null);if(!at)return;event.preventDefault();lastPoint.current=at;surface.current.focus();if(down)event.currentTarget.setPointerCapture(event.pointerId);client.media?.sendInput({type:'button',...at,button:event.button,down});};
  const keyboard=(event,down)=>{if(!allowed||event.isComposing)return;if(event.key==='Escape'){client.media?.sendInput({type:'release'});return;}event.preventDefault();client.media?.sendInput({type:'key',code:event.code,down});};
  const metrics=remote.metrics||{};
  return <div className={`remote-video-viewport ${scale==='actual'?'actual':''}`}>
    <Surface ref={surface} {...(directCanvas?{}:{autoPlay:true,muted:true,playsInline:true})} tabIndex={allowed?0:-1} aria-label="对方共享画面" className={allowed?'controllable':''} style={scale==='actual'?{width:metrics.width||session.screen?.width,height:metrics.height||session.screen?.height}:undefined}
      onPointerDown={event=>pointer(event,true)} onPointerUp={event=>pointer(event,false)} onPointerCancel={()=>client.media?.sendInput({type:'release'})}
      onPointerMove={event=>{if(!allowed)return;const at=point(event);if(!at)return;lastPoint.current=at;move.current=at;if(!frame.current)frame.current=requestAnimationFrame(()=>{frame.current=0;if(move.current)client.media?.sendInput({type:'move',...move.current});});}}
      onKeyDown={event=>keyboard(event,true)} onKeyUp={event=>keyboard(event,false)} onBlur={()=>client.media?.sendInput({type:'release'})} onContextMenu={event=>{if(allowed)event.preventDefault();}}/>
    {session.paused?<div className="remote-video-cover paused"><RemoteIcon name="pause" size={36}/><b>对方已暂停共享</b><p>等待对方继续共享，鼠标和键盘控制已收回。</p></div>:!directCanvas&&!stream?.getVideoTracks().length&&<div className="remote-video-cover"><RemoteIcon size={44}/><b>正在连接共享画面…</b><p>连接完成后显示对方选择的屏幕。</p></div>}
  </div>;
}
