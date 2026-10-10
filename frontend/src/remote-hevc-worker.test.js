import test from "node:test";
import assert from "node:assert/strict";
import {createHevcWorkerRuntime,validHevcStreamDescriptor} from "./remote-hevc-worker.js";
import {createRemoteHevcWorkerSender} from "./remote-hevc-worker-client.js";
import {HEVC_CHANNEL} from "./remote-hevc.js";
import {RemoteHevcMedia,createHevcControlOutbox} from "./remote-hevc-media.js";
import {RemoteMedia} from "./remote-media.js";

const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const until=async predicate=>{for(let i=0;i<150;i++){if(predicate())return;await wait(2);}assert.fail("condition timed out");};
const descriptor={url:"ws://127.0.0.1:18888/api/remote/native/stream",token:"a".repeat(64),format:"hevc-v1"};
const capability={type:"xchat-hevc",version:1,sessionId:"session",action:"capability",supported:true};
function packet(sequence=1) {
  const bytes=new Uint8Array(73),view=new DataView(bytes.buffer);
  for(const [offset,value]of [[0,0x31564858],[4,72],[8,1920],[12,1080],[16,2560],[20,1440],[40,1000],[44,2000],[48,3],[52,1],[60,30],[64,33000]])view.setUint32(offset,value,true);
  view.setUint16(56,4,true);bytes.set(new TextEncoder().encode("test"),68);
  view.setBigUint64(24,BigInt(sequence),true);view.setBigUint64(32,BigInt(sequence*33333),true);return bytes.buffer;
}
function runtimeHarness({sendFrame,now,limits}={}) {
  const events=[],sockets=[],sent=[],controls=[];let runtime,frames=0,activeRevision=1;
  class Socket extends EventTarget {
    constructor(url){super();this.url=url;this.requests=[];this.closed=false;this.revision=activeRevision;sockets.push(this);queueMicrotask(()=>this.dispatchEvent(new Event("open")));}
    send(value){const message=JSON.parse(value);this.requests.push(message);if(message.type==="auth")queueMicrotask(()=>this.text({type:"ready",format:"hevc-v1",revision:this.revision??1}));}
    close(){this.closed=true;this.dispatchEvent(new Event("close"));}
    text(data){this.dispatchEvent(new MessageEvent("message",{data:JSON.stringify(data)}));}
    binary(data=packet()){this.dispatchEvent(new MessageEvent("message",{data}));}
  }
  let sender;
  runtime=createHevcWorkerRuntime({WebSocket:Socket,now,postMessage:event=>events.push(event),createSender:options=>{
    sender={supported:false,ready:false,failed:false,handleControl(value){controls.push(value);if(value.action==="capability")this.supported=value.supported;},
      async sendFrame(frame,scope){sent.push({frame,scope});if(sendFrame)await sendFrame(frame);frames++;this.ready=true;options.onReady(this.stats());return true;},
      stats(){return {framesSent:frames,hardwareEncoder:true};},reset(){this.ready=false;},close(){this.ready=false;}};return sender;
  }});
  runtime.message({type:"init",sessionId:"session",channel:{label:HEVC_CHANNEL},limits});
  const claim=(epoch,revision)=>runtime.message({type:"capture",epoch,revision});
  const update=(epoch,revision,paused=false,capture=true)=>{
    activeRevision=revision;runtime.message({type:"update",epoch,revision,paused});
    if(capture&&!paused)claim(epoch,revision);
  };
  const authorize=async(revision=1)=>{
    const old=sockets.length;await until(()=>events.some(event=>event.type==="open-stream"&&event.revision===revision));
    const request=events.filter(event=>event.type==="open-stream"&&event.revision===revision).at(-1);
    runtime.message({type:"stream",epoch:request.epoch,requestId:request.requestId,descriptor});
    await until(()=>sockets.length>old);sockets.at(-1).revision=revision;
    await until(()=>sockets.at(-1).requests.some(message=>message.type==="next"));return sockets.at(-1);
  };
  return {runtime,events,sockets,sent,update,claim,authorize,get sender(){return sender;},
    start({capture=true}={}){update(1,1,false,capture);runtime.message({type:"control",epoch:1,control:capability});},close:()=>runtime.close()};
}

test("native stream descriptor accepts only the scoped localhost route and never query tokens",()=>{
  assert.equal(validHevcStreamDescriptor(descriptor),true);
  for(const url of ["ws://192.168.1.1:18888/api/remote/native/stream","wss://127.0.0.1:18888/api/remote/native/stream",
    descriptor.url+"?token=secret","ws://user:secret@127.0.0.1:18888/api/remote/native/stream",descriptor.url+"/other"]) {
    assert.equal(validHevcStreamDescriptor({...descriptor,url}),false);
  }
  assert.equal(validHevcStreamDescriptor({...descriptor,token:"bad"}),false);
});

test("one exclusive capture claim drives repeated native reads without any further window ticks",async()=>{
  const h=runtimeHarness();
  try {
    h.start();const socket=await h.authorize();
    for(let i=1;i<=5;i++){socket.binary(packet(i));await until(()=>socket.requests.filter(value=>value.type==="next").length===i+1);}
    assert.equal(h.sent.length,5);assert.equal(h.events.filter(value=>value.type==="open-stream").length,1);
    assert.equal(socket.requests[0].token,descriptor.token);assert.equal(socket.requests[1].request_keyframe,true);
    assert.equal(socket.requests[2].request_keyframe,false);assert.equal(h.sent[0].scope.revision,1);
  }finally{h.close();}
});

test("generic capability does not race the Window's outstanding RGBA read",async()=>{
  const h=runtimeHarness();
  try {
    h.start({capture:false});await wait(15);
    assert.equal(h.events.some(value=>value.type==="open-stream"),false);
    h.claim(1,1);await h.authorize();h.claim(1,1);await wait(5);
    assert.equal(h.events.filter(value=>value.type==="open-stream").length,1);
  }finally{h.close();}
});

test("a missing native authorization descriptor has a bounded fallback and rejects late credentials",async()=>{
  const h=runtimeHarness({limits:{descriptorTimeoutMs:25,firstFrameTimeoutMs:150}});
  try {
    h.start();const request=h.events.find(value=>value.type==="open-stream");
    await until(()=>h.events.some(value=>value.type==="fallback"));
    const failure=h.events.find(value=>value.type==="fallback");
    assert.match(failure.reason,/authorization descriptor timed out/);
    assert.equal(failure.diagnostics.phase,"failed");
    h.runtime.message({type:"stream",epoch:1,requestId:request.requestId,descriptor});await wait(5);
    assert.equal(h.sockets.length,0);
    assert.equal(JSON.stringify(h.events).includes(descriptor.token),false);
    assert.equal(JSON.stringify(h.events).includes(descriptor.url),false);
  }finally{h.close();}
});

test("the first decoded frame has a total deadline even when a negotiation Promise never settles",async()=>{
  let release;const gate=new Promise(resolve=>{release=resolve;});
  const h=runtimeHarness({sendFrame:()=>gate,limits:{firstFrameTimeoutMs:40}});
  try {
    h.start();const socket=await h.authorize();socket.binary();await until(()=>h.sent.length===1);
    await until(()=>h.events.some(value=>value.type==="fallback"));
    assert.match(h.events.find(value=>value.type==="fallback").reason,/first decoded frame timed out/);
    assert.equal(socket.closed,true);release();await wait(5);
    assert.equal(h.events.some(value=>value.type==="ready"),false);
    assert.equal(socket.requests.filter(value=>value.type==="next").length,1);
  }finally{release();h.close();}
});

test("actual first-frame output cancels the startup deadline for a static desktop",async()=>{
  const h=runtimeHarness({limits:{firstFrameTimeoutMs:40}});
  try {
    h.start();const socket=await h.authorize();socket.binary();
    await until(()=>h.events.some(value=>value.type==="ready"));await wait(65);
    assert.equal(h.sender.ready,true);assert.equal(h.events.some(value=>value.type==="fallback"),false);
  }finally{h.close();}
});

test("a local socket close between native reads retains its terminal error and triggers fallback",async()=>{
  let release;const gate=new Promise(resolve=>{release=resolve;});const h=runtimeHarness({sendFrame:()=>gate});
  try {
    h.start();const socket=await h.authorize();socket.binary();await until(()=>h.sent.length===1);
    socket.text({type:"error",code:"remote_hevc_encode",error:"hardware encoder timed out"});
    assert.equal(socket.closed,true);release();
    await until(()=>h.events.some(value=>value.type==="fallback"));
    assert.equal(h.events.find(value=>value.type==="fallback").reason,"hardware encoder timed out");
    assert.equal(h.events.filter(value=>value.type==="fallback").length,1);
    assert.equal(h.events.some(value=>value.type==="invalidated"),false);
  }finally{release();h.close();}
});

test("native demand stays bounded while the previous encoded frame waits for configuration/output ACK",async()=>{
  let release;const gate=new Promise(resolve=>{release=resolve;});const h=runtimeHarness({sendFrame:()=>gate});
  try {
    h.start();const socket=await h.authorize();socket.binary();await until(()=>h.sent.length===1);await wait(15);
    assert.equal(socket.requests.filter(value=>value.type==="next").length,1);
    release();await until(()=>socket.requests.filter(value=>value.type==="next").length===2);
  }finally{release();h.close();}
});

test("pause invalidates a pending native read and delayed old bytes cannot reach the sender",async()=>{
  const h=runtimeHarness();
  try {
    h.start();const socket=await h.authorize();h.update(2,2,true);socket.binary();await wait(10);
    assert.equal(socket.closed,true);assert.equal(h.sent.length,0);assert.equal(h.events.filter(value=>value.type==="fallback").length,0);
    h.update(3,3);const next=await h.authorize(3);next.binary();await until(()=>h.sent.length===1);
    assert.equal(h.sent[0].scope.revision,3);assert.equal(next.requests[1].request_keyframe,true);
  }finally{h.close();}
});

test("an authorization descriptor resolved after pause cannot open any socket",async()=>{
  const h=runtimeHarness();
  try {
    h.start();const request=h.events.find(value=>value.type==="open-stream");h.update(2,2,true);
    h.runtime.message({type:"stream",epoch:1,requestId:request.requestId,descriptor});await wait(10);
    assert.equal(h.sockets.length,0);assert.equal(h.events.filter(value=>value.type==="fallback").length,0);
  }finally{h.close();}
});

test("pause cancels startup deadlines and a new revision requires a new exclusive claim",async()=>{
  const h=runtimeHarness({limits:{descriptorTimeoutMs:30,firstFrameTimeoutMs:40}});
  try {
    h.start();h.update(2,2,true);await wait(60);
    assert.equal(h.events.some(value=>value.type==="fallback"),false);
    h.update(3,3,false,false);await wait(5);
    assert.equal(h.events.filter(value=>value.type==="open-stream").length,1);
    h.claim(3,3);const socket=await h.authorize(3);socket.binary();
    await until(()=>h.events.some(value=>value.type==="ready"&&value.epoch===3));
  }finally{h.close();}
});

test("busy retries the same authorized socket with an explicit five-second cap",async()=>{
  let time=100;const h=runtimeHarness({now:()=>time});
  try {
    h.start();const socket=await h.authorize();socket.text({type:"error",code:"remote_capture_busy",retryable:true,error:"busy"});
    await until(()=>socket.requests.filter(value=>value.type==="next").length===2);
    assert.equal(socket.requests.at(-1).request_keyframe,true);assert.equal(h.sockets.length,1);
    time+=5001;socket.text({type:"error",code:"remote_capture_busy",retryable:true,error:"busy"});
    await until(()=>h.events.some(value=>value.type==="fallback"));assert.equal(socket.closed,true);
    assert.equal(h.events.filter(value=>value.type==="open-stream").length,1);
  }finally{h.close();}
});

test("a changed native revision waits for authoritative update instead of reusing its token or failing HEVC",async()=>{
  const h=runtimeHarness();
  try {
    h.start();const socket=await h.authorize();socket.text({type:"error",code:"remote_revision_changed",error:"changed"});
    await until(()=>h.events.some(value=>value.type==="invalidated"));assert.equal(socket.closed,true);
    assert.equal(h.events.filter(value=>value.type==="fallback").length,0);assert.equal(h.events.filter(value=>value.type==="open-stream").length,1);
    h.update(2,2);await h.authorize(2);assert.equal(h.sockets.length,2);
  }finally{h.close();}
});

test("authorization failures stop native reads and report a single explicit fallback",async()=>{
  const h=runtimeHarness();
  try {
    h.start();const socket=await h.authorize();socket.text({type:"error",code:"remote_stream_unauthorized",error:"denied"});
    await until(()=>h.events.some(value=>value.type==="fallback"));await wait(30);
    assert.equal(h.events.filter(value=>value.type==="fallback").length,1);assert.equal(h.sockets.length,1);assert.equal(h.sent.length,0);
    assert.equal(h.events.filter(value=>value.type==="control"&&value.control.action==="fallback").length,1);
  }finally{h.close();}
});

class WorkerMock extends EventTarget {
  static instances=[];
  constructor(){super();this.messages=[];this.terminated=false;WorkerMock.instances.push(this);}
  postMessage(message,transfer){this.messages.push(message);if(transfer)transfer[0].readyState="closed";}
  terminate(){this.terminated=true;}
  deliver(data){this.dispatchEvent(new MessageEvent("message",{data}));}
}
function proxyHarness(overrides={}) {
  const channel={label:HEVC_CHANNEL,readyState:"connecting"},controls=[],fallbacks=[];
  const proxy=createRemoteHevcWorkerSender({pc:{sctp:{maxMessageSize:1200},createDataChannel:()=>channel},sessionId:"session",
    openStream:async()=>descriptor,Worker:WorkerMock,sendControl:message=>controls.push(message),onFallback:reason=>fallbacks.push(reason),...overrides});
  return {proxy,channel,worker:WorkerMock.instances.at(-1),controls,fallbacks,close(){proxy.close();this.worker.deliver({type:"closed"});}};
}

test("a synchronous transfer failure reuses the untransferred channel for the existing sender",()=>{
  class Unsupported extends WorkerMock {postMessage(){throw new DOMException("unsupported","DataCloneError");}}
  let fallbackChannel;const expected={legacy:true};
  const h=proxyHarness({Worker:Unsupported,createSender:options=>{fallbackChannel=options.pc.createDataChannel();return expected;}});
  assert.equal(h.proxy,expected);assert.equal(fallbackChannel,h.channel);assert.equal(h.worker.terminated,true);
});

test("window proxy drops old authorization results and old readiness after revision changes",async()=>{
  let release;const h=proxyHarness({openStream:()=>new Promise(resolve=>{release=resolve;})});
  try {
    h.proxy.update({revision:1,paused:false});const capture=h.proxy.capture(1);
    h.worker.deliver({type:"open-stream",epoch:1,requestId:1,revision:1});await until(()=>release);
    h.proxy.update({revision:2,paused:true});release(descriptor);await wait(5);
    assert.equal(await capture,false);
    h.worker.deliver({type:"ready",epoch:1,metrics:{framesSent:5}});
    assert.equal(h.proxy.ready,false);assert.equal(h.worker.messages.some(value=>value.type==="stream"),false);
    assert.equal(h.proxy.ownsCapture,true);assert.equal(h.channel.readyState,"closed");
  }finally{h.close();}
});

test("worker initialization has a finite deadline that releases the legacy capture loop",async()=>{
  const h=proxyHarness({limits:{workerInitTimeoutMs:25,firstFrameTimeoutMs:150}});
  try {
    h.proxy.update({revision:1,paused:false});h.proxy.handleControl(capability);
    const capture=h.proxy.capture(1);
    assert.equal(await capture,false);assert.equal(h.proxy.failed,true);assert.equal(h.worker.terminated,true);
    assert.match(h.fallbacks[0],/initialization timed out/);
    assert.equal(h.proxy.stats().hevcWorker.phase,"failed");
  }finally{h.close();}
});

test("initialization ACK from epoch zero remains valid after the immediate epoch-one update",async()=>{
  const h=proxyHarness({limits:{workerInitTimeoutMs:20}});
  try {
    h.proxy.update({revision:1,paused:false});h.worker.deliver({type:"initialized",epoch:0});await wait(40);
    assert.equal(h.proxy.stats().hevcWorker.initialized,true);assert.equal(h.proxy.failed,false);
    assert.equal(h.proxy.ready,false);assert.equal(h.fallbacks.length,0);
  }finally{h.close();}
});

test("late same-epoch metrics cannot revoke capability or the exclusive HEVC capture claim",async()=>{
  const h=proxyHarness();
  try {
    h.proxy.update({revision:1,paused:false});h.worker.deliver({type:"initialized",epoch:0});
    h.proxy.handleControl(capability);const capture=h.proxy.capture(1);
    // This update snapshot was emitted before Worker received capability, but
    // arrives after Window has accepted capability and stopped its RGBA reads.
    h.worker.deliver({type:"metrics",epoch:1,metrics:{},state:{supported:false,ready:false,failed:false},
      diagnostics:{phase:"waiting-capability",phaseAgeMs:1,lastProgressAgeMs:1,captureOwned:false}});
    assert.equal(h.proxy.supported,true);assert.equal(h.proxy.stats().hevcWorker.captureOwned,true);
    assert.equal(await capture,true);
    assert.equal(h.worker.messages.filter(value=>value.type==="capture").length,1);
    // Nor may old statistics restore capability after a real peer withdrawal.
    h.proxy.handleControl({...capability,supported:false});
    h.worker.deliver({type:"metrics",epoch:1,metrics:{},state:{supported:true,ready:false,failed:false}});
    assert.equal(h.proxy.supported,false);
  }finally{h.close();}
});

test("a never-resolving openStream invoke expires and its late result cannot revive capture",async()=>{
  let release;const h=proxyHarness({limits:{descriptorTimeoutMs:25,firstFrameTimeoutMs:150},
    openStream:()=>new Promise(resolve=>{release=resolve;})});
  try {
    h.proxy.update({revision:1,paused:false});h.worker.deliver({type:"initialized",epoch:0});h.proxy.handleControl(capability);
    const capture=h.proxy.capture(1);
    h.worker.deliver({type:"open-stream",epoch:1,revision:1,requestId:1});await until(()=>release);
    assert.equal(await capture,false);assert.match(h.fallbacks[0],/authorization descriptor timed out/);
    release(descriptor);await wait(5);
    assert.equal(h.worker.messages.some(value=>value.type==="stream"),false);
    assert.equal(JSON.stringify(h.proxy.stats()).includes(descriptor.token),false);
    assert.equal(JSON.stringify(h.proxy.stats()).includes(descriptor.url),false);
  }finally{h.close();}
});

test("repeated capture ticks cannot extend first-frame negotiation indefinitely",async()=>{
  const h=proxyHarness({limits:{firstFrameTimeoutMs:40}});
  try {
    h.proxy.update({revision:1,paused:false});h.worker.deliver({type:"initialized",epoch:0});h.proxy.handleControl(capability);
    const pending=[h.proxy.capture(1)];
    for(let i=0;i<5;i++){await wait(10);pending.push(h.proxy.capture(1));}
    assert.equal(h.proxy.failed,true);assert.match(h.fallbacks[0],/first decoded frame timed out/);
    assert.deepEqual(await Promise.all(pending),Array(pending.length).fill(false));
    assert.equal(h.worker.messages.filter(value=>value.type==="capture").length,1);
  }finally{h.close();}
});

test("ready cancels the Window startup watchdog without requiring further motion or metrics",async()=>{
  const h=proxyHarness({limits:{firstFrameTimeoutMs:30}});
  try {
    h.proxy.update({revision:1,paused:false});h.worker.deliver({type:"initialized",epoch:0});h.proxy.handleControl(capability);
    const capture=h.proxy.capture(1);h.worker.deliver({type:"ready",epoch:1,metrics:{framesSent:1}});
    await wait(55);assert.equal(h.proxy.failed,false);assert.equal(h.proxy.ready,true);assert.equal(h.fallbacks.length,0);
    h.proxy.close();assert.equal(await capture,false);
  }finally{h.close();}
});

test("native scope invalidation releases capture ownership until an authoritative update",async()=>{
  const h=proxyHarness();
  try {
    h.proxy.update({revision:1,paused:false});h.worker.deliver({type:"initialized",epoch:0});h.proxy.handleControl(capability);
    const capture=h.proxy.capture(1);h.worker.deliver({type:"invalidated",epoch:1,code:"remote_revision_changed"});
    assert.equal(await capture,false);assert.equal(await h.proxy.capture(1),false);assert.equal(h.proxy.failed,false);
    assert.equal(h.proxy.stats().hevcWorker.captureOwned,false);
    h.proxy.update({revision:2,paused:false});const resumed=h.proxy.capture(2);
    assert.equal(h.worker.messages.filter(value=>value.type==="capture").length,2);
    h.proxy.close();assert.equal(await resumed,false);
  }finally{h.close();}
});

test("closing cancels all startup watchdogs without a late fallback",async()=>{
  const h=proxyHarness({limits:{workerInitTimeoutMs:20,firstFrameTimeoutMs:20}});
  h.proxy.update({revision:1,paused:false});const capture=h.proxy.capture(1);h.close();
  assert.equal(await capture,false);await wait(40);assert.equal(h.fallbacks.length,0);
});

test("a failed Worker cannot reclaim capture and the real legacy loop keeps presenting later RGBA frames",async()=>{
  const h=proxyHarness(),originalImageData=Object.getOwnPropertyDescriptor(globalThis,"ImageData");
  let reads=0,releases=0,painted=0,requested=0;
  const samples=[1,2,3,4,null,null,5,6];
  const media={closed:false,native:true,session:{revision:1,paused:false},
    canvas:{width:2,height:2,getContext:()=>({putImageData(){painted++;}})},
    localScreen:{getVideoTracks:()=>[{requestFrame(){requested++;}}]},
    fail(error){throw error;},
    frameReader:{async read(){
      const sequence=samples[reads++],unchanged=sequence===null;
      const bytes=new Uint8Array(44+(unchanged?0:16)),view=new DataView(bytes.buffer);
      bytes.set(new TextEncoder().encode("XRF1"));view.setUint16(4,44,true);bytes[6]=1;bytes[7]=2;
      for(const offset of [8,12,28,32])view.setUint32(offset,2,true);
      view.setBigUint64(16,BigInt(sequence??4),true);view.setUint32(36,unchanged?1:0,true);
      return {bytes,transport:"test",release(){releases++;if(reads===samples.length)media.closed=true;}};
    }},
  };
  media.hevc=Object.assign(Object.create(RemoteHevcMedia.prototype),{closed:false,sender:h.proxy,media});
  try {
    Object.defineProperty(globalThis,"ImageData",{configurable:true,value:class {constructor(pixels,width,height){Object.assign(this,{pixels,width,height});}}});
    h.proxy.update(media.session);h.worker.deliver({type:"initialized",epoch:0});h.proxy.handleControl(capability);
    const capture=h.proxy.capture(1);
    h.worker.deliver({type:"fallback",epoch:1,reason:"remote_hevc_unsupported: DXGI initial frame unavailable"});
    assert.equal(await capture,false);assert.equal(h.proxy.failed,true);assert.equal(h.worker.terminated,true);
    const messages=h.worker.messages.length;
    h.proxy.handleControl(capability);h.proxy.reset();h.proxy.update({revision:2,paused:false});
    h.worker.deliver({type:"ready",epoch:1,metrics:{framesSent:1}});
    assert.equal(await h.proxy.capture(1),false);assert.equal(h.worker.messages.length,messages);
    await RemoteMedia.prototype.captureLoop.call(media);
    assert.equal(reads,8);assert.equal(releases,8);assert.equal(painted,6);assert.equal(requested,6);
    assert.equal(h.proxy.stats().hevcWorker.captureOwned,false);assert.equal(h.proxy.ready,false);
    assert.equal(media.captureMetrics.captureBackend,"GDI");
  }finally{
    if(originalImageData)Object.defineProperty(globalThis,"ImageData",originalImageData);else delete globalThis.ImageData;
    h.close();
  }
});

test("a healthy Worker survives normal control metadata while the input channel is being replaced",async()=>{
  const delivered=[],media={session:{id:'session'},channel:{readyState:'closed',bufferedAmount:0},inputSuspended:true};
  const outbox=createHevcControlOutbox(media),h=proxyHarness({sendControl:outbox.send});
  try {
    h.proxy.update({revision:1,paused:false});h.worker.deliver({type:'initialized',epoch:0});h.proxy.handleControl(capability);
    const capture=h.proxy.capture(1);h.worker.deliver({type:'ready',epoch:1,metrics:{framesSent:20}});
    for(let frames=21;frames<=30;frames++)h.worker.deliver({type:'control',epoch:1,
      control:{...capability,action:'metrics',streamId:'stream',metadata:{frames}}});
    assert.equal(h.worker.messages.some(message=>message.type==='fail'),false);
    assert.equal(h.proxy.failed,false);assert.equal(h.proxy.ready,true);assert.equal(await capture,true);
    media.channel={readyState:'open',bufferedAmount:0,send:value=>delivered.push(JSON.parse(value))};outbox.flush();
    assert.equal(delivered.length,1);assert.equal(delivered[0].metadata.frames,30);
    assert.equal(media.inputSuspended,true);assert.equal(h.fallbacks.length,0);
  }finally{outbox.close();h.close();}
});

test("proxy capture yields the old loop without requesting frames, and close settles its wait",async()=>{
  const h=proxyHarness();
  try {
    const pending=h.proxy.capture(1);assert.equal(h.worker.messages.filter(value=>value.type==="update").length,1);
    assert.equal(h.worker.messages.some(value=>value.type==="next"),false);
    h.proxy.close();assert.equal(await pending,false);assert.equal(h.proxy.ready,false);
  }finally{h.close();}
});

test("an old capture callback cannot undo an authoritative pause or roll its revision back",async()=>{
  const h=proxyHarness();
  try {
    h.proxy.update({revision:2,paused:true});const messages=h.worker.messages.length;
    assert.equal(await h.proxy.capture(1),false);assert.equal(await h.proxy.capture(2),false);
    h.proxy.update({revision:1,paused:false});
    assert.equal(h.worker.messages.length,messages);assert.equal(h.proxy.ready,false);
  }finally{h.close();}
});
