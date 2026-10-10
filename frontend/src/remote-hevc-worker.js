import {createRemoteHevcSender,decodeNativeHevcFrame,HEVC_CHANNEL} from "./remote-hevc.js";

const clock=()=>globalThis.performance?.now?.()??Date.now();
const cancelled=()=>Object.assign(new Error("HEVC worker operation cancelled"),{cancelled:true});
const validRevision=value=>Number.isSafeInteger(value)&&value>=0;
const deadline=(value,fallback)=>Number.isSafeInteger(value)&&value>0&&value<=fallback?value:fallback;

export function validHevcStreamDescriptor(value) {
  try {
    const url=new URL(value?.url);
    return value.format==="hevc-v1"&&/^[0-9a-f]{64}$/.test(value.token)
      &&url.protocol==="ws:"&&url.hostname==="127.0.0.1"&&Number(url.port)>0
      &&url.pathname==="/api/remote/native/stream"&&!url.search&&!url.hash&&!url.username&&!url.password;
  }catch{return false;}
}

// All per-frame native reads and SCTP sends live here. Window messages carry
// only authorization descriptors, low-rate metrics, and reliable peer control.
export function createHevcWorkerRuntime({postMessage,WebSocket:Socket=globalThis.WebSocket,
  createSender=createRemoteHevcSender,now=clock}={}) {
  let sender=null,closed=false,failed=false,epoch=0,generation=0,revision=null,paused=true;
  let running=false,connection=null,descriptorWaiter=null,serial=0,needKey=true,captureOwned=false;
  let lastMetrics=-Infinity,extra={},sctp={maxMessageSize:59999},sessionId=null;
  let phase="initializing",phaseAt=now(),lastProgress=phaseAt,nativeRequests=0,nativeResponses=0;
  let descriptorTimeout=5000,firstFrameTimeout=15000,firstFrameTimer=null,metricsTimer=null;
  const emit=(type,fields={})=>postMessage({type,epoch,...fields});
  const current=token=>!closed&&!failed&&!paused&&token===generation;
  const diagnostics=()=>({phase,phaseAgeMs:Math.max(0,now()-phaseAt),lastProgressAgeMs:Math.max(0,now()-lastProgress),
    nativeRequests,nativeResponses,running,captureOwned});
  function stage(value,progress=false){if(phase!==value){phase=value;phaseAt=now();}if(progress)lastProgress=now();}
  function clearFirstFrame(){clearTimeout(firstFrameTimer);firstFrameTimer=null;}
  function awaitFirstFrame(){
    if(firstFrameTimer||!captureOwned||paused||closed||failed)return;
    const token=generation;
    firstFrameTimer=setTimeout(()=>{
      firstFrameTimer=null;
      if(current(token)&&!sender?.ready)fail(new Error("HEVC first decoded frame timed out"));
    },firstFrameTimeout);
    firstFrameTimer.unref?.();
  }
  function publish(force=false) {
    if(!sender||(!force&&now()-lastMetrics<250))return;
    lastMetrics=now();emit("metrics",{metrics:{...sender.stats(),...extra},
      state:{supported:sender.supported,ready:sender.ready,failed:sender.failed},diagnostics:diagnostics()});
  }
  function cancelTransport() {
    ++generation;captureOwned=false;clearFirstFrame();
    if(descriptorWaiter){clearTimeout(descriptorWaiter.timer);descriptorWaiter.reject(cancelled());descriptorWaiter=null;}
    connection?.stop();connection=null;
  }
  function fail(error,notify=true) {
    if(closed||failed)return;
    failed=true;cancelTransport();sender?.close();clearInterval(metricsTimer);stage("failed");
    const reason=String(error?.message??error).slice(0,256);
    if(notify&&sessionId)emit("control",{control:{type:"xchat-hevc",version:1,sessionId,action:"fallback",reason}});
    emit("fallback",{reason,diagnostics:diagnostics()});
  }
  function descriptor(token) {
    stage("authorizing");
    return new Promise((resolve,reject)=>{
      const requestId=++serial,active={requestId,token,resolve,reject};descriptorWaiter=active;
      active.timer=setTimeout(()=>{
        if(descriptorWaiter!==active)return;
        descriptorWaiter=null;reject(new Error("HEVC native capture authorization descriptor timed out"));
      },descriptorTimeout);
      active.timer.unref?.();
      emit("open-stream",{requestId,revision,diagnostics:diagnostics()});
    });
  }
  function connect(value,token) {
    if(!validHevcStreamDescriptor(value))throw new Error("HEVC 本地采集授权地址无效");
    stage("connecting",true);
    const socket=new Socket(value.url);socket.binaryType="arraybuffer";
    let ready=false,stopped=false,terminalError=null,waiter=null,readyResolve,readyReject;
    const opened=new Promise((resolve,reject)=>{readyResolve=resolve;readyReject=reject;});
    const authTimer=setTimeout(()=>stop(new Error("HEVC 本地采集授权超时")),5000);
    function finish(error,value) {
      if(!waiter)return;
      const active=waiter;waiter=null;clearTimeout(active.timer);
      if(error)active.reject(error);else active.resolve(value);
    }
    function stop(error=cancelled()) {
      if(stopped)return;stopped=true;terminalError=error;clearTimeout(authTimer);
      readyReject(error);finish(error);
      try{socket.close();}catch{/* already closed */}
    }
    socket.addEventListener("open",()=>{
      if(!current(token)){stop();return;}
      stage("authenticating",true);
      socket.send(JSON.stringify({type:"auth",token:value.token}));
    });
    socket.addEventListener("message",event=>{
      if(stopped||!current(token)){stop();return;}
      if(event.data instanceof ArrayBuffer) {
        if(!ready||!waiter||event.data.byteLength>4*1024*1024+1024){stop(new Error("HEVC 本地采集响应超出请求边界"));return;}
        finish(null,{buffer:event.data});return;
      }
      let message;
      try {if(typeof event.data!=="string"||event.data.length>4096)throw 0;message=JSON.parse(event.data);}catch{stop(new Error("HEVC 本地采集响应格式无效"));return;}
      if(message.type==="ready"&&!ready&&message.format==="hevc-v1"&&message.revision===revision){
        ready=true;clearTimeout(authTimer);stage("authorized",true);readyResolve();return;
      }
      if(message.type==="error") {
        if(message.code==="remote_capture_busy"&&message.retryable===true&&ready&&waiter){finish(null,{busy:true});return;}
        stop(Object.assign(new Error(String(message.error??message.code??"HEVC 本地采集失败").slice(0,256)),{code:message.code}));return;
      }
      stop(new Error("HEVC 本地采集响应与状态不匹配"));
    });
    socket.addEventListener("error",()=>stop(new Error("HEVC 本地采集连接失败")));
    socket.addEventListener("close",()=>stop(new Error("HEVC 本地采集连接已关闭")));
    return {opened,stop,next(requestKeyframe){
      if(!current(token))return Promise.reject(cancelled());
      // A close between native reads must not masquerade as revision/close
      // cancellation and silently abandon the only active frame pump.
      if(stopped)return Promise.reject(terminalError);
      if(!ready||waiter)return Promise.reject(new Error("HEVC native capture request state is invalid"));
      return new Promise((resolve,reject)=>{
        waiter={resolve,reject,timer:setTimeout(()=>stop(new Error("HEVC 本地采集帧超时")),10000)};
        try{socket.send(JSON.stringify({type:"next",request_keyframe:requestKeyframe}));}
        catch(error){stop(error);}
      });
    }};
  }
  async function pump() {
    if(running||closed||failed||paused||!captureOwned||!validRevision(revision)||!sender?.supported)return;
    running=true;const token=generation;
    try {
      const access=await descriptor(token);if(!current(token))return;
      const stream=connect(access,token);connection=stream;
      await stream.opened;if(!current(token))return;
      needKey=true;let busySince=null;
      while(current(token)) {
        const requestKeyframe=needKey;needKey=false;const started=now();
        stage("capturing");nativeRequests++;
        const response=await stream.next(requestKeyframe);if(!current(token))return;
        if(response.busy){
          busySince??=now();
          if(now()-busySince>=5000)throw new Error("HEVC 本地采集持续忙碌，已停止请求");
          stage("capture-busy",true);needKey ||= requestKeyframe;await new Promise(resolve=>setTimeout(resolve,25));continue;
        }
        busySince=null;nativeResponses++;stage("frame-received",true);
        const frame=decodeNativeHevcFrame(response.buffer);
        extra={captureMs:frame.captureMs,encodeMs:frame.encodeMs,nativeMs:frame.nativeMs,
          bridgeMs:frame.nativeMs===null?null:Math.max(0,now()-started-frame.nativeMs),
          captureBackend:frame.captureBackend,sourceWidth:frame.sourceWidth,sourceHeight:frame.sourceHeight,
          captureTransport:"worker-websocket",requestedFPS:frame.requestedFPS};
        if(!frame.unchanged){
          stage(sender.ready?"sending":"awaiting-decoder");
          await sender.sendFrame(frame,{revision});
        }else needKey ||= requestKeyframe;
        if(!current(token))return;
        publish();
      }
    } catch(error) {
      if(!error?.cancelled&&current(token)) {
        if(["remote_paused","remote_revision_changed","remote_session_ended"].includes(error.code)) {
          paused=true;cancelTransport();sender.reset();stage("awaiting-session-update");
          emit("invalidated",{code:error.code,diagnostics:diagnostics()});publish(true);
        }else fail(error);
      }
    }
    finally {running=false;if(!closed&&!failed&&!paused&&token!==generation)void pump();}
  }
  function update(message) {
    if(!validRevision(message.revision)||!Number.isSafeInteger(message.epoch)||message.epoch<epoch)return;
    epoch=message.epoch;revision=message.revision;paused=message.paused===true;
    cancelTransport();sender?.reset();needKey=true;
    stage(paused?"paused":sender?.supported?"waiting-capture":"waiting-capability",true);publish(true);
  }
  function message(value) {
    if(!value||typeof value!=="object"||closed)return;
    if(value.type==="init"&&!sender) {
      if(typeof value.sessionId!=="string"||value.sessionId.length>256||value.channel?.label!==HEVC_CHANNEL){fail(new Error("HEVC worker 初始化无效"));return;}
      sessionId=value.sessionId;sctp.maxMessageSize=value.maxMessageSize??59999;
      descriptorTimeout=deadline(value.limits?.descriptorTimeoutMs,5000);
      firstFrameTimeout=deadline(value.limits?.firstFrameTimeoutMs,15000);
      sender=createSender({sessionId:value.sessionId,pc:{sctp,createDataChannel:()=>value.channel},limits:value.limits,
        sendControl:control=>{if(control.action==="config")awaitFirstFrame();emit("control",{control});return true;},requestKeyFrame:()=>{needKey=true;},
        onReady:metrics=>{
          if(closed||failed||paused||!captureOwned)return;
          clearFirstFrame();stage("streaming",true);emit("ready",{metrics:{...metrics,...extra},diagnostics:diagnostics()});publish(true);
        },
        onFallback:reason=>fail(reason,false)});
      stage("waiting-capability",true);metricsTimer=setInterval(()=>publish(),250);metricsTimer.unref?.();
      emit("initialized",{diagnostics:diagnostics()});return;
    }
    if(value.type==="close"){closed=true;cancelTransport();sender?.close();clearInterval(metricsTimer);stage("closed");emit("closed");return;}
    if(!sender||failed)return;
    if(value.type==="update"||value.type==="reset"){update(value);return;}
    if(value.epoch!==epoch)return;
    if(value.type==="capture") {
      if(paused||value.revision!==revision||captureOwned)return;
      // The Window grants ownership only after its previous RGBA read has
      // settled. Capability discovery alone must not race two native formats.
      captureOwned=true;awaitFirstFrame();void pump();
    }else if(value.type==="control") {
      if(Number.isFinite(value.maxMessageSize)&&value.maxMessageSize>48)sctp.maxMessageSize=value.maxMessageSize;
      const control=value.control;
      if(control?.type==="xchat-hevc"&&control.action==="reset"&&validRevision(control.revision)&&control.revision>=revision
          &&(control.revision!==revision||control.paused===true)) {
        paused=true;cancelTransport();
        stage("awaiting-session-update");
      }
      sender.handleControl(control);
      if(!captureOwned&&!paused)stage(sender.supported?"waiting-capture":"waiting-capability");
      if(!sender.ready)awaitFirstFrame();
      publish(control?.action==="capability"||control?.action==="reset");void pump();
    }else if(value.type==="stream"&&descriptorWaiter?.requestId===value.requestId) {
      const active=descriptorWaiter;descriptorWaiter=null;clearTimeout(active.timer);
      if(value.error)active.reject(new Error(String(value.error).slice(0,256)));
      else active.resolve(value.descriptor);
    }else if(value.type==="fail")fail(value.reason);
  }
  return {message,close:()=>message({type:"close"})};
}

if(typeof DedicatedWorkerGlobalScope!=="undefined"&&globalThis instanceof DedicatedWorkerGlobalScope) {
  const runtime=createHevcWorkerRuntime({postMessage:value=>globalThis.postMessage(value)});
  globalThis.addEventListener("message",event=>runtime.message(event.data));
}
