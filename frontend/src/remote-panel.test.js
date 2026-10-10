import test from 'node:test';
import assert from 'node:assert/strict';
import {clampPanel,newPanelMemory,panelStatus} from './remote-panel.js';
import {createNativePanelAdapter,nearestMonitor} from './remote-native-panel.js';

test('panel positions clamp inside a viewport and negative-origin monitor work area',()=>{
  assert.deepEqual(clampPanel({x:-50,y:900},{width:200,height:44},{width:390,height:600}),{x:8,y:548});
  assert.deepEqual(clampPanel({x:-4000,y:-500},{width:600,height:80},{x:-1920,y:-120,width:1920,height:1080},0),{x:-1920,y:-120});
  assert.notEqual(newPanelMemory(),newPanelMemory());
  assert.equal(panelStatus({local_host:true,grant:'grant'},true),'对方正在控制');
  assert.equal(panelStatus({local_host:true,paused:true,grant:null},true),'共享已暂停');
});

function nativeFixture() {
  const monitor={position:{x:-1600,y:-200},size:{width:1600,height:1000},scaleFactor:1.5,workArea:{position:{x:-1600,y:-180},size:{width:1600,height:940}}};
  let point={x:-1200,y:0},size={},moveHandler;
  const calls=[],errors=[];
  class Position {constructor(x,y){this.x=x;this.y=y;}}
  class Size {constructor(width,height){this.width=width;this.height=height;}}
  const win={
    outerPosition:async()=>({...point}),scaleFactor:async()=>1.5,
    setSize:async value=>{size=value;calls.push(['size',value]);},
    setPosition:async value=>{assert.ok(Number.isInteger(value.x)&&Number.isInteger(value.y),'Tauri physical coordinates must be integers');point=value;calls.push(['position',value]);},
    startDragging:async()=>{point={x:-900,y:200};},
    onMoved:async fn=>{moveHandler=fn;return()=>{moveHandler=null;};},
  };
  const api={getCurrentWindow:()=>win,availableMonitors:async()=>[monitor],currentMonitor:async()=>monitor,PhysicalPosition:Position,LogicalSize:Size};
  return {api,monitor,calls,errors,get point(){return point;},get size(){return size;},get listening(){return Boolean(moveHandler);}};
}
test('native fold keeps independent positions and does not call hide or minimize',async()=>{
  const f=nativeFixture(),adapter=createNativePanelAdapter(f.api,e=>f.errors.push(e));
  const full={isConnected:true,offsetWidth:501,parentElement:{offsetHeight:80}};
  await adapter.layout(full,false);await adapter.idle();
  const original={...f.point};
  assert.equal(f.size.width,533);assert.equal(f.point.y,-180);
  await adapter.layout({isConnected:true,offsetWidth:180,parentElement:{offsetHeight:76}},true);await adapter.idle();
  assert.equal(f.size.width,212);
  await adapter.drag();assert.equal(f.point.x,-900);
  await adapter.layout(full,false);await adapter.idle();
  assert.deepEqual({...f.point},original);
  await adapter.nudge([1,0],8);assert.equal(f.point.x,original.x+12);
  await adapter.nudge(null,8);assert.deepEqual({...f.point},original);
  await adapter.layout({isConnected:true,offsetWidth:180,parentElement:{offsetHeight:76}},true);await adapter.idle();
  assert.deepEqual({...f.point},original,'fold starts at the full panel, matching the inline prototype');
  assert.deepEqual(f.errors,[]);
  adapter.dispose();await adapter.nudge([1,0],8);assert.equal(f.listening,false);
});
test('native layout coalesces rapid collapse/restore and ignores disposed work',async()=>{
  const f=nativeFixture(),adapter=createNativePanelAdapter(f.api,e=>f.errors.push(e));
  const node={isConnected:true,offsetWidth:500,parentElement:{offsetHeight:80}};
  void adapter.layout(node,false);void adapter.layout({...node,offsetWidth:180},true);void adapter.layout(node,false);
  await adapter.idle();assert.equal(f.size.width,532);
  adapter.dispose();const count=f.calls.length;await adapter.layout(node,true);await adapter.idle();assert.equal(f.calls.length,count);
  assert.deepEqual(f.errors,[]);
  assert.equal(nearestMonitor({x:-1700,y:0},[f.monitor]).position.x,-1600);
});
