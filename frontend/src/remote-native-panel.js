import {clampPanel} from './remote-panel.js';

export function nearestMonitor(point,monitors) {
  return [...monitors].sort((a,b)=>{
    const distance=m=>{
      const area=m.workArea||m,left=area.position.x,top=area.position.y;
      return Math.hypot(Math.max(left-point.x,0,point.x-left-area.size.width),Math.max(top-point.y,0,point.y-top-area.size.height));
    };
    return distance(a)-distance(b);
  })[0];
}

// Only the independent, visible sharing window uses this adapter. No hide/minimize APIs.
export function createNativePanelAdapter(api,onError=()=>{}) {
  const win=api.getCurrentWindow(),positions={full:null,compact:null};
  let alive=true,tail=Promise.resolve(),mode=null,applied='',requested=null,scheduled=false,applying=false,timer=0,unlisten=[];
  const attempt=fn=>{tail=tail.catch(()=>{}).then(async()=>{if(alive)await fn();}).catch(e=>{if(alive)onError(String(e.message||e));});return tail;};
  // Kept as a separate function so every transition uses the same physical work-area bounds.
  const fit=async(size,preferred,reset=false)=>{
    const monitors=await api.availableMonitors(),current=await api.currentMonitor();
    const monitor=reset?(current||monitors[0]):nearestMonitor(preferred,monitors)||current;
    if(!monitor)throw Error('无法确定共享工具栏所在显示器');
    const area=monitor.workArea||monitor,scale=monitor.scaleFactor||1;
    const physical={width:Math.ceil(size.width*scale),height:Math.ceil(size.height*scale)};
    const point=clampPanel(reset?{x:area.position.x+(area.size.width-physical.width)/2,y:area.position.y}:preferred,physical,{x:area.position.x,y:area.position.y,width:area.size.width,height:area.size.height},0);
    // Tauri physical coordinates are i32, including centers on fractional DPI scales.
    return {x:Math.round(point.x),y:Math.round(point.y)};
  };
  const settle=()=>attempt(async()=>{
    if(!requested||applying)return;
    const actual=await win.outerPosition(),point=await fit(requested.size,actual);
    if(!alive)return;
    if(point.x!==actual.x||point.y!==actual.y){applying=true;try{await win.setPosition(new api.PhysicalPosition(point.x,point.y));}finally{applying=false;}}
    positions[mode||'full']=point;
  });
  function schedule() {
    if(scheduled||!alive)return;
    scheduled=true;
    return attempt(async()=>{
      scheduled=false;
      const next=requested;if(!next)return;
      const signature=`${next.mode}:${next.size.width}:${next.size.height}`;
      if(signature===applied)return;
      const actual=await win.outerPosition();
      if(!alive)return;
      if(mode&&mode!==next.mode){positions[mode]=actual;if(next.mode==='compact')positions.compact=actual;}
      const point=await fit(next.size,mode===next.mode?actual:positions[next.mode]||actual,mode===null);
      if(!alive)return;
      applying=true;
      try {
        await win.setSize(new api.LogicalSize(next.size.width,next.size.height));
        if(!alive)return;
        await win.setPosition(new api.PhysicalPosition(point.x,point.y));
        mode=next.mode;positions[mode]=point;applied=signature;
      } finally {applying=false;}
      if(requested!==next)schedule();
    });
  }
  const onMove=()=>{if(applying||!alive)return;clearTimeout(timer);timer=setTimeout(()=>void settle(),160);};
  const listen=promise=>promise.then(off=>{if(alive)unlisten.push(off);else off();}).catch(e=>{if(alive)onError(String(e));});
  listen(win.onMoved(onMove));
  if(win.onScaleChanged)listen(win.onScaleChanged(()=>{applied='';void schedule();}));
  return {
    layout(node,collapsed) {
      if(!alive||!node.isConnected)return;
      requested={mode:collapsed?'compact':'full',size:{width:Math.ceil(node.offsetWidth)+32,height:Math.ceil(node.parentElement.offsetHeight)}};
      return schedule();
    },
    async drag() {if(!alive)return;try{await win.startDragging();if(alive)await settle();}catch(e){if(alive)onError(String(e));}},
    nudge(delta,step) {return attempt(async()=>{
      if(!requested)return;
      const actual=await win.outerPosition(),scale=await win.scaleFactor();
      const preferred=delta?{x:actual.x+delta[0]*step*scale,y:actual.y+delta[1]*step*scale}:actual;
      const point=await fit(requested.size,preferred,!delta);
      if(!alive)return;
      applying=true;try{await win.setPosition(new api.PhysicalPosition(point.x,point.y));positions[mode||'full']=point;}finally{applying=false;}
    });},
    dispose(){alive=false;clearTimeout(timer);unlisten.forEach(off=>off());unlisten=[];},
    idle:()=>tail,
  };
}
