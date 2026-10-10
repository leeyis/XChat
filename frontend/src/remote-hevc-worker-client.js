import {createRemoteHevcSender,HEVC_CHANNEL} from "./remote-hevc.js";

const validRevision=value=>Number.isSafeInteger(value)&&value>=0;
const clock=()=>globalThis.performance?.now?.()??Date.now();
const deadline=(value,fallback)=>Number.isSafeInteger(value)&&value>0&&value<=fallback?value:fallback;
const parseControl=(value,sessionId)=>{
  try {
    if(typeof value==="string"){if(value.length>4096)return null;value=JSON.parse(value);}
    return value?.type==="xchat-hevc"&&value.version===1&&value.sessionId===sessionId&&JSON.stringify(value).length<=4096?value:null;
  }catch{return null;}
};

// The transferred channel must leave the creating task before its first send.
// A synchronous transfer failure leaves it usable by the existing sender.
export function createRemoteHevcWorkerSender(options) {
  const {pc,sessionId,openStream,sendControl,onReady=()=>{},onFallback=()=>{},onMetrics,
    Worker:WorkerClass,createSender=createRemoteHevcSender}=options;
  if(!(WorkerClass??globalThis.Worker)||typeof openStream!=="function")return createSender(options);
  let worker,channel;
  try {
    worker=WorkerClass?new WorkerClass("./remote-hevc-worker.js",{type:"module",name:"xchat-hevc-capture"})
      :new Worker(new URL("./remote-hevc-worker.js",import.meta.url),{type:"module",name:"xchat-hevc-capture"});
    channel=pc.createDataChannel(HEVC_CHANNEL,{ordered:false,maxRetransmits:0});
    worker.postMessage({type:"init",channel,sessionId,maxMessageSize:pc.sctp?.maxMessageSize,limits:options.limits},[channel]);
  }catch(error) {
    worker?.terminate();
    if(channel&&channel.readyState==="closed"){
      // A detached channel cannot safely be reclaimed. Explicit RTP fallback.
      queueMicrotask(()=>onFallback("HEVC worker 通道转移失败"));
      return {ownsCapture:false,supported:false,ready:false,failed:true,handleControl:()=>false,
        sendFrame:async()=>false,stats:()=>({}),reset(){},close(){},fail(){}};
    }
    const fallbackPC=channel?{get sctp(){return pc.sctp;},createDataChannel:()=>channel}:pc;
    return createSender({...options,pc:fallbackPC});
  }
  let closed=false,failed=false,supported=false,ready=false,epoch=0,revision=null,paused=true;
  let initialized=false,captureOwned=false,invalidated=false,failureReason="";
  let metrics={},terminationTimer=null,firstFrameTimer=null,authorization=null,workerDiagnostic=null,diagnosticAt=clock();
  let phase="initializing",phaseAt=clock(),lastProgress=phaseAt;
  const initTimeout=deadline(options.limits?.workerInitTimeoutMs,5000);
  const descriptorTimeout=deadline(options.limits?.descriptorTimeoutMs,5000);
  const firstFrameTimeout=deadline(options.limits?.firstFrameTimeoutMs,15000);
  const captureWaiters=new Set();
  const post=value=>{if(!closed&&!failed)worker.postMessage({epoch,...value});};
  function releaseCaptures(){for(const resolve of [...captureWaiters])resolve(false);captureWaiters.clear();}
  function stage(value){phase=value;phaseAt=clock();workerDiagnostic=null;}
  function receiveDiagnostic(value){if(value){workerDiagnostic=value;diagnosticAt=clock();}}
  function clearFirstFrame(){clearTimeout(firstFrameTimer);firstFrameTimer=null;}
  function clearAuthorization(){if(authorization)clearTimeout(authorization.timer);authorization=null;}
  function awaitFirstFrame(){
    if(firstFrameTimer||ready||!captureOwned||paused||invalidated||closed||failed)return;
    const requestEpoch=epoch;
    firstFrameTimer=setTimeout(()=>{
      firstFrameTimer=null;
      if(requestEpoch===epoch&&!ready)fail("HEVC first decoded frame timed out");
    },firstFrameTimeout);
    firstFrameTimer.unref?.();
  }
  function stats(){
    const elapsed=clock()-diagnosticAt;
    const diagnostic=workerDiagnostic?{...workerDiagnostic,phaseAgeMs:workerDiagnostic.phaseAgeMs+elapsed,
      lastProgressAgeMs:workerDiagnostic.lastProgressAgeMs+elapsed}:
      {phase,phaseAgeMs:clock()-phaseAt,lastProgressAgeMs:clock()-lastProgress};
    return {...metrics,hevcWorker:{...diagnostic,initialized,captureOwned,epoch,revision,paused,invalidated,
      supported,ready,failed,failureReason}};
  }
  function fail(reason,notify=true) {
    if(closed||failed)return;failed=true;ready=false;captureOwned=false;
    clearTimeout(initTimer);clearFirstFrame();clearAuthorization();releaseCaptures();worker.terminate();stage("failed");
    const message=String(reason?.message??reason).slice(0,256);
    failureReason=message;
    if(notify)try{sendControl({type:"xchat-hevc",version:1,sessionId,action:"fallback",reason:message});}catch{/* closing */}
    onFallback(message);
  }
  const initTimer=setTimeout(()=>fail("HEVC worker initialization timed out"),initTimeout);
  initTimer.unref?.();
  worker.addEventListener("error",event=>fail(event.message??"HEVC worker 加载失败"));
  worker.addEventListener("messageerror",()=>fail("HEVC worker 消息转移失败"));
  worker.addEventListener("message",event=>{
    const message=event.data;
    if(message?.type==="closed"){clearTimeout(terminationTimer);worker.terminate();return;}
    // init is posted before the constructor's immediate session update. Its
    // ACK can legitimately carry epoch 0 while the Window is already epoch 1.
    if(message?.type==="initialized"){
      if(closed||failed)return;
      initialized=true;clearTimeout(initTimer);lastProgress=clock();
      if(phase==="initializing")stage(paused?"paused":supported?"waiting-capture":"waiting-capability");
      return;
    }
    if(closed||failed||message?.epoch!==epoch)return;
    receiveDiagnostic(message.diagnostics);
    if(message.type==="control") {
      if(message.control?.action==="config"){ready=false;awaitFirstFrame();}
      try {if(sendControl(message.control)===false)post({type:"fail",reason:"HEVC 可靠控制通道未就绪"});}
      catch(error){post({type:"fail",reason:String(error?.message??error).slice(0,256)});}
    }else if(message.type==="open-stream") {
      const requestEpoch=epoch;
      if(paused||invalidated||!captureOwned||message.revision!==revision)return;
      if(authorization){fail("HEVC native capture authorization request overlaps");return;}
      const active={epoch:requestEpoch,requestId:message.requestId};authorization=active;
      active.timer=setTimeout(()=>{
        if(authorization!==active)return;
        authorization=null;fail("HEVC native capture authorization descriptor timed out");
      },descriptorTimeout);
      active.timer.unref?.();
      Promise.resolve().then(()=>openStream(message.revision)).then(descriptor=>{
        if(authorization!==active)return;
        clearAuthorization();
        if(!closed&&!failed&&requestEpoch===epoch&&!paused&&!invalidated)post({type:"stream",requestId:message.requestId,descriptor});
      },error=>{
        if(authorization!==active)return;
        clearAuthorization();
        if(!closed&&!failed&&requestEpoch===epoch)post({type:"stream",requestId:message.requestId,error:String(error?.message??error).slice(0,256)});
      });
    }else if(message.type==="ready") {ready=true;clearFirstFrame();metrics={...metrics,...message.metrics};onReady(stats());}
    else if(message.type==="metrics") {
      // Capability is authoritative on the reliable Window control channel.
      // A same-epoch snapshot posted before that message may arrive after our
      // exclusive capture claim; it must not send the Window back into RGBA.
      metrics=message.metrics;ready=message.state.ready;
      if(ready)clearFirstFrame();else awaitFirstFrame();
      onMetrics?.(stats());
    }else if(message.type==="invalidated"){
      ready=false;invalidated=true;captureOwned=false;clearFirstFrame();clearAuthorization();releaseCaptures();
    }
    else if(message.type==="fallback")fail(message.reason,false);
  });
  function update(next) {
    if(closed||failed||!validRevision(next?.revision)||(revision!==null&&next.revision<revision))return;
    const nextPaused=next.paused===true;
    if(revision===next.revision&&paused===nextPaused)return;
    revision=next.revision;paused=nextPaused;ready=false;captureOwned=false;invalidated=false;++epoch;
    clearFirstFrame();clearAuthorization();releaseCaptures();
    stage(!initialized?"initializing":paused?"paused":supported?"waiting-capture":"waiting-capability");
    post({type:"update",revision,paused});
  }
  function reset(next) {
    if(next){update(next);return;}
    if(closed||failed)return;
    ready=false;captureOwned=false;invalidated=false;++epoch;clearFirstFrame();clearAuthorization();releaseCaptures();
    stage(paused?"paused":"waiting-capture");
    if(validRevision(revision))post({type:"reset",revision,paused});
  }
  function handleControl(value) {
    const control=parseControl(value,sessionId);if(!control)return false;
    if(control.action==="capability"&&typeof control.supported==="boolean")supported=control.supported;
    if(control.action==="reset"){
      ready=false;
      if(validRevision(control.revision)&&control.revision>=revision&&(control.revision!==revision||control.paused===true)){
        invalidated=true;captureOwned=false;clearFirstFrame();clearAuthorization();releaseCaptures();stage("awaiting-session-update");
      }else awaitFirstFrame();
    }
    post({type:"control",control,maxMessageSize:pc.sctp?.maxMessageSize});return true;
  }
  async function capture(nextRevision) {
    if(closed||failed)return false;
    if(revision===null)update({revision:nextRevision,paused:false});
    if(paused||invalidated||revision!==nextRevision)return false;
    if(!captureOwned){
      captureOwned=true;stage(initialized?"starting":"initializing");awaitFirstFrame();
      post({type:"capture",revision});
    }
    // Only the surrounding legacy loop waits here. Worker capture never awaits
    // document timers, rendering, metrics delivery, or another capture() call.
    return new Promise(resolve=>{
      const done=value=>{clearTimeout(timer);captureWaiters.delete(done);resolve(value);};
      const requestEpoch=epoch;
      const timer=setTimeout(()=>done(!closed&&!failed&&!invalidated&&captureOwned&&requestEpoch===epoch),100);captureWaiters.add(done);
    });
  }
  function close() {
    if(closed)return;
    closed=true;ready=false;captureOwned=false;clearTimeout(initTimer);clearFirstFrame();clearAuthorization();
    releaseCaptures();stage("closed");worker.postMessage({type:"close",epoch});
    terminationTimer=setTimeout(()=>worker.terminate(),1000);terminationTimer.unref?.();
  }
  return {ownsCapture:true,get supported(){return supported;},get ready(){return ready;},get failed(){return failed;},
    update,reset,capture,handleControl,stats,sendFrame:async()=>false,close,fail};
}
