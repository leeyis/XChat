import {useEffect,useLayoutEffect,useRef,useState} from 'react';
import {clampPanel,panelStatus,readPanelCollapsed,writePanelCollapsed,watchPanelCollapsed} from './remote-panel.js';
import './remote-controls.css';

export function RemoteIcon({name='screen',size=18}) {
  const paths={
    screen:<><rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8M12 17v4"/></>,
    control:<><rect x="2" y="3" width="20" height="14" rx="2"/><path d="M7 21h5M10 17v4m4-11 7 4-3 1-1 3-3-8Z"/></>,
    help:<><path d="M8 7H4v12h16V7h-4M9 22h6M12 19v3"/><path d="M12 2v11m-4-4 4 4 4-4"/></>,
    expand:<path d="M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5"/>,
    shrink:<path d="M3 8h5V3m8 0v5h5M8 21v-5H3m13 5v-5h5"/>,
    pause:<path d="M8 5v14M16 5v14"/>,play:<path d="m8 4 12 8-12 8Z"/>,
    tune:<path d="M4 6h16M4 12h16M4 18h16M8 3v6m8 0v6m-6 0v6"/>,
    send:<path d="m3 3 18 9-18 9 4-9-4-9Zm4 9h14"/>,
    mic:<><rect x="9" y="2" width="6" height="13" rx="3"/><path d="M5 10v2a7 7 0 0 0 14 0v-2M12 19v3m-4 0h8"/></>,
    micOff:<path d="m3 3 18 18M9 9v3a3 3 0 0 0 5 2M9 5a3 3 0 0 1 6 0v6M5 10v2a7 7 0 0 0 12 5M19 10v2M12 19v3m-4 0h8"/>,
    sound:<path d="M3 9h4l5-5v16l-5-5H3Z M16 8a6 6 0 0 1 0 8m3-11a10 10 0 0 1 0 14"/>,
    soundOff:<path d="M3 9h4l5-5v16l-5-5H3Zm13 0 6 6m0-6-6 6"/>,
    phone:<path d="M7 3H3v3c0 8 7 15 15 15h3v-4l-5-2-2 2a16 16 0 0 1-7-7l2-2Z"/>,
    hangup:<path d="M3 15v-4a17 17 0 0 1 18 0v4h-5v-4a14 14 0 0 0-8 0v4Z"/>,
    shield:<><path d="M12 2 3 6v6c0 5 9 10 9 10s9-5 9-10V6Z"/><path d="m8 12 3 3 5-6"/></>,
    chat:<path d="M5 4h14a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H9l-5 3v-3H3V6a2 2 0 0 1 2-2Z"/>,
    close:<path d="m6 6 12 12M6 18 18 6"/>,hide:<path d="M5 12h14"/>,
    grip:<>{[5,12,19].map(y=><g key={y}><circle cx="9" cy={y} r="1"/><circle cx="15" cy={y} r="1"/></g>)}</>,
  };
  return <svg className="ra-icon" viewBox="0 0 24 24" width={size} height={size} fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[name]||paths.screen}</svg>;
}
export function RemoteTool({icon,children,className='',...props}) {
  return <button type="button" className={`ra-button ra-tool ${className}`} {...props}>{icon&&<RemoteIcon name={icon}/>}<span>{children}</span></button>;
}

export function SharingControls({session,busy,onRevoke,onPause,onEnd,grip,hide,hidden}) {
  return <div className={`ra-sharing-strip ra-panel-tools ${session?.grant?'control':''}`} hidden={hidden}>
    {grip}<RemoteIcon name={session?.grant?'control':'screen'}/><strong title={panelStatus(session)}>{panelStatus(session)}</strong>
    <div>{session?.grant&&<RemoteTool icon="shield" className="danger" disabled={busy} onClick={onRevoke}>收回控制</RemoteTool>}
      <RemoteTool icon={session?.paused?'play':'pause'} disabled={busy||session?.phase!=='active'} onClick={onPause}>{session?.paused?'继续共享':'暂停共享'}</RemoteTool>
      <RemoteTool icon="close" className="danger" onClick={onEnd} aria-label="结束协助">结束</RemoteTool>
    </div>{hide}
  </div>;
}

export function RemoteFloatingPanel({session,memory,busy,onRevoke,onEnd,beforeInteract,children,nativeAdapter}) {
  const [collapsed,setCollapsed]=useState(()=>readPanelCollapsed(session.id));
  const panel=useRef(null),drag=useRef(null),placeRef=useRef(()=>{}),focusAfter=useRef(null),previous=useRef(collapsed);
  const host=session.local_host;
  useEffect(()=>watchPanelCollapsed(session.id,setCollapsed),[session.id]);
  useLayoutEffect(()=>{
    const node=panel.current,stage=node.parentElement;
    const field=()=>collapsed?'compactPosition':'position';
    const current=()=>({x:parseFloat(node.style.left)||0,y:parseFloat(node.style.top)||0});
    if(previous.current!==collapsed&&!nativeAdapter&&collapsed){memory.position=current();memory.compactPosition={...memory.position};}
    previous.current=collapsed;
    const place=(point=memory[field()])=>{
      if(nativeAdapter){void nativeAdapter.layout(node,collapsed);return;}
      if(!stage.clientWidth||!stage.clientHeight)return;
      const size={width:node.offsetWidth,height:node.offsetHeight};
      const position=clampPanel(point||{x:(stage.clientWidth-size.width)/2,y:(stage.querySelector('.ra-viewer-status')?.offsetHeight||0)+12},size,{width:stage.clientWidth,height:stage.clientHeight});
      node.style.left=`${position.x}px`;node.style.top=`${position.y}px`;return position;
    };
    placeRef.current=place;place();
    const resize=new ResizeObserver(()=>place());resize.observe(stage);resize.observe(node);
    if(focusAfter.current){node.querySelector(focusAfter.current)?.focus({preventScroll:true});focusAfter.current=null;}
    return()=>resize.disconnect();
  },[collapsed,memory,nativeAdapter]);
  const finish=()=>{
    const id=drag.current?.id;drag.current=null;panel.current?.classList.remove('dragging');
    if(id!==undefined&&panel.current?.hasPointerCapture(id))panel.current.releasePointerCapture(id);
  };
  useEffect(()=>{window.addEventListener('blur',finish);return()=>{finish();window.removeEventListener('blur',finish);};},[]);
  const toggle=()=>{beforeInteract?.();focusAfter.current=collapsed?'.ra-panel-hide':'.ra-panel-restore';writePanelCollapsed(session.id,!collapsed);setCollapsed(!collapsed);};
  const pointerDown=event=>{
    if(event.button!==0||event.isPrimary===false)return;
    event.preventDefault();event.stopPropagation();beforeInteract?.();event.currentTarget.focus({preventScroll:true});
    if(nativeAdapter){void nativeAdapter.drag();return;}
    drag.current={id:event.pointerId,startX:event.clientX,startY:event.clientY,x:parseFloat(panel.current.style.left)||0,y:parseFloat(panel.current.style.top)||0};
    panel.current.setPointerCapture(event.pointerId);panel.current.classList.add('dragging');
  };
  const keyboard=event=>{
    const delta={ArrowLeft:[-1,0],ArrowRight:[1,0],ArrowUp:[0,-1],ArrowDown:[0,1]}[event.key];
    if(!delta&&event.key!=='Home')return;
    event.preventDefault();event.stopPropagation();beforeInteract?.();
    if(nativeAdapter){void nativeAdapter.nudge(delta,event.shiftKey?24:8);return;}
    const field=collapsed?'compactPosition':'position';
    if(event.key==='Home'){memory[field]=null;placeRef.current();return;}
    const step=event.shiftKey?24:8;
    memory[field]=placeRef.current({x:(parseFloat(panel.current.style.left)||0)+delta[0]*step,y:(parseFloat(panel.current.style.top)||0)+delta[1]*step});
  };
  const grip=<button type="button" className="ra-panel-grip" aria-label="拖动工具栏" title="拖动调整位置；方向键微调，Home 复位" onPointerDown={pointerDown} onKeyDown={keyboard}><RemoteIcon name="grip"/></button>;
  const hide=<RemoteTool icon="hide" className="ra-panel-hide" title="隐藏工具栏，保留恢复浮标" onClick={toggle}>隐藏</RemoteTool>;
  const status=panelStatus(session,true);
  return <section ref={panel} className={`ra-floating-panel ${host?'host':'viewer'} ${collapsed?'collapsed':''}`} aria-label={host?'屏幕共享控制栏':'远控工具栏'} onPointerMove={event=>{
    const start=drag.current;if(!start||start.id!==event.pointerId)return;event.preventDefault();event.stopPropagation();
    memory[collapsed?'compactPosition':'position']=placeRef.current({x:start.x+event.clientX-start.startX,y:start.y+event.clientY-start.startY});
  }} onPointerUp={finish} onPointerCancel={finish} onLostPointerCapture={finish}>
    {children({grip,hide,hidden:collapsed})}
    <div className={`ra-panel-compact ${session.grant?'control':''} ${session.paused?'paused':''}`} hidden={!collapsed}>
      {grip}<button type="button" className="ra-panel-restore" title="展开工具栏" aria-label={`${status}，展开工具栏`} onClick={toggle}><i className="ra-panel-dot"/><span>{status}</span><RemoteIcon name="expand"/></button>
      {host&&session.grant&&<RemoteTool icon="shield" className="ra-compact-action danger" aria-label="收回控制" title="收回控制" disabled={busy} onClick={onRevoke}>收回控制</RemoteTool>}
      <RemoteTool icon="close" className="ra-compact-action danger" aria-label="结束协助" title="结束协助" onClick={onEnd}>结束协助</RemoteTool>
    </div>
  </section>;
}
