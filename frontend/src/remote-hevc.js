// Native HEVC access units over a bounded, unreliable SCTP stream. The reliable
// control channel is supplied by RemoteMedia; this module never creates a PC.
export const HEVC_CHANNEL = "xchat-hevc-video-v1";

const CONTROL = "xchat-hevc", VERSION = 1, HEADER = 48;
const MAX_FRAME = 4 * 1024 * 1024, MAX_BUFFER = 4 * 1024 * 1024;
const MAX_PENDING = 3, MAX_REASSEMBLY = 8 * 1024 * 1024;
const FRAME_TIMEOUT = 180, CONFIG_TIMEOUT = 5000, MAX_DECODE = 3;
const now = () => globalThis.performance?.now?.() ?? Date.now();
const bytesOf = value => value instanceof ArrayBuffer ? new Uint8Array(value)
  : ArrayBuffer.isView(value) ? new Uint8Array(value.buffer,value.byteOffset,value.byteLength) : null;
const integer = (value,min,max) => Number.isSafeInteger(value) && value >= min && value <= max;
const short = (value,max=256) => typeof value === "string" && value.length <= max ? value : null;
const codecValid = value => typeof value === "string" && value.length <= 96 && /^hev1\.[A-C]?\d+\.[0-9a-f]+\.[LH]\d+(?:\.[0-9a-f]{1,2}){0,6}$/i.test(value);
const epochValid = value => typeof value === "string" && /^[0-9a-f]{16}$/.test(value);
const dimension = value => integer(value,1,16384);
const codedSize = (width,height) => dimension(width)&&dimension(height)&&width*height<=3840*2160;
const safeUs = value => value <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(value) : null;

export function decodeNativeHevcFrame(value) {
  const bytes=bytesOf(value);
  if (!bytes || bytes.length < 64) throw new Error("HEVC 原生帧头不完整");
  const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);
  if (view.getUint32(0,true)!==0x31564858) throw new Error("HEVC 原生帧格式无效");
  const headerLength=view.getUint32(4,true), nameLength=view.getUint16(56,true);
  const fixedLength=headerLength-nameLength, payloadLength=view.getUint32(52,true), flags=view.getUint32(48,true);
  const width=view.getUint32(8,true), height=view.getUint32(12,true);
  const sourceWidth=view.getUint32(16,true), sourceHeight=view.getUint32(20,true);
  const timestampUs=safeUs(view.getBigUint64(32,true)), requestedFPS=view.getUint32(60,true);
  if (![64,68].includes(fixedLength) || nameLength>512 || headerLength>bytes.length
      || payloadLength>MAX_FRAME || headerLength+payloadLength!==bytes.length
      || !codedSize(width,height) || ![sourceWidth,sourceHeight].every(dimension) || timestampUs===null
      || flags & ~7 || Boolean(flags&4)!==(payloadLength===0) || !integer(requestedFPS,1,240)) {
    throw new Error("HEVC 原生帧参数无效");
  }
  const encoderImplementation=new TextDecoder("utf-8",{fatal:true}).decode(bytes.subarray(fixedLength,headerLength));
  if (!short(encoderImplementation)) throw new Error("HEVC 编码器名称过长");
  return {width,height,codedWidth:width,codedHeight:height,sourceWidth,sourceHeight,
    sequence:view.getBigUint64(24,true).toString(),timestampUs,requestedFPS,
    captureMs:view.getUint32(40,true)/1000,encodeMs:view.getUint32(44,true)/1000,
    nativeMs:fixedLength>=68?view.getUint32(64,true)/1000:null,
    keyframe:Boolean(flags&1),hardwareEncoder:Boolean(flags&2),unchanged:Boolean(flags&4),
    encoderImplementation,captureBackend:"DXGI",data:bytes.subarray(headerLength)};
}

function annexBNals(value) {
  const bytes=bytesOf(value); if (!bytes) return [];
  const starts=[];
  for (let i=0;i+3<=bytes.length;i++) {
    if (bytes[i]===0 && bytes[i+1]===0 && bytes[i+2]===1) {starts.push([i,i+3]);i+=2;}
    else if (i+4<=bytes.length && bytes[i]===0 && bytes[i+1]===0 && bytes[i+2]===0 && bytes[i+3]===1) {starts.push([i,i+4]);i+=3;}
  }
  return starts.map((entry,index)=>bytes.subarray(entry[1],starts[index+1]?.[0]??bytes.length)).filter(nal=>nal.length>=2);
}

export function hevcCodecFromAnnexB(value) {
  const nal=annexBNals(value).find(nal=>((nal[0]>>1)&63)===33);
  if (!nal) return null;
  const rbsp=[];
  for(let i=2;i<nal.length;i++) {
    if (i>=4 && nal[i]===3 && nal[i-1]===0 && nal[i-2]===0) continue;
    rbsp.push(nal[i]);
  }
  if (rbsp.length<13) return null;
  const profile=rbsp[1], space=profile>>6, tier=(profile&32)?"H":"L";
  let compatibility=0;
  for(let bit=0;bit<32;bit++) if (rbsp[2+(bit>>3)]&(1<<(7-(bit&7)))) compatibility+=(2**bit);
  const constraints=rbsp.slice(6,12);
  while(constraints.length>1 && constraints.at(-1)===0)constraints.pop();
  return `hev1.${space?"ABC"[space-1]:""}${profile&31}.${compatibility.toString(16).toUpperCase()}.${tier}${rbsp[12]}.${constraints.map(value=>value.toString(16).toUpperCase()).join(".")}`;
}

function independentKey(data) {
  const types=new Set(annexBNals(data).map(nal=>(nal[0]>>1)&63));
  return [32,33,34].every(type=>types.has(type)) && [...types].some(type=>type>=16&&type<=23);
}
function parseControl(value,sessionId) {
  try {
    if (typeof value==="string") {if(value.length>4096)return null;value=JSON.parse(value);}
    if (!value || typeof value!=="object" || Array.isArray(value)
        || value.type!==CONTROL || value.version!==VERSION || value.sessionId!==sessionId
        || JSON.stringify(value).length>4096) return null;
    return value;
  } catch {return null;}
}
function controlSender(sessionId,sendControl) {
  return (action,fields={}) => {
    try {return sendControl({type:CONTROL,version:VERSION,sessionId,action,...fields})!==false;}
    catch {return false;}
  };
}
function newEpoch() {
  const value=new Uint8Array(8);globalThis.crypto.getRandomValues(value);
  return [...value].map(n=>n.toString(16).padStart(2,"0")).join("");
}
function nativeMetadata(frame) {
  return {encoderImplementation:short(frame.encoderImplementation)??"",hardwareEncoder:frame.hardwareEncoder===true,
    sourceWidth:frame.sourceWidth,sourceHeight:frame.sourceHeight,captureMs:frame.captureMs,encodeMs:frame.encodeMs,nativeMs:frame.nativeMs};
}
function readMetadata(value) {
  if (!value || typeof value!=="object")return {};
  const result={};
  if(short(value.encoderImplementation)!==null)result.encoderImplementation=value.encoderImplementation;
  if(typeof value.hardwareEncoder==="boolean")result.hardwareEncoder=value.hardwareEncoder;
  for(const key of ["sourceWidth","sourceHeight"])if(dimension(value[key]))result[key]=value[key];
  for(const key of ["captureMs","encodeMs","nativeMs"])if(Number.isFinite(value[key])&&value[key]>=0&&value[key]<60000)result[key]=value[key];
  return result;
}
function metricsCounter(role,onMetrics) {
  const data={transport:"dtls-sctp",codec:"H265",codecString:null,hardwareDecoder:null,
    powerEfficientEncoder:null,powerEfficientDecoder:null,lossPercent:null,
    framesEncoded:0,framesSent:0,framesReceived:0,framesDecoded:0,framesDropped:0,
    bytesSent:0,bytesReceived:0,fragmentsSent:0,fragmentsReceived:0,invalidPackets:0,duplicatePackets:0,
    keyframeRequests:0,decodeMs:null,receiveToDecodeMs:null};
  let previous=null,lastNotice=-Infinity;
  const snapshot=()=>{
    const at=now(), frames=role==="sender"?data.framesSent:data.framesDecoded, bytes=role==="sender"?data.bytesSent:data.bytesReceived;
    const seconds=previous?(at-previous.at)/1000:0;
    const result={...data,fps:seconds>0?(frames-previous.frames)/seconds:null,kbps:seconds>0?(bytes-previous.bytes)*8/seconds/1000:null};
    previous={at,frames,bytes};return result;
  };
  return {data,snapshot,notice:()=>{if(onMetrics&&now()-lastNotice>=1000){lastNotice=now();onMetrics({...data});}}};
}
function packetSize(pc,requested) {
  const negotiated=pc.sctp?.maxMessageSize;
  return Math.min(59999,integer(requested,HEADER+1,59999)?requested:59999,
    Number.isFinite(negotiated)&&negotiated>0?Math.floor(negotiated):59999);
}
function makePacket(streamId,sequence,frame,index,count,stride) {
  const offset=index*stride,length=Math.min(stride,frame.data.length-offset), bytes=new Uint8Array(HEADER+length), view=new DataView(bytes.buffer);
  view.setUint32(0,0x31434858,true);view.setUint8(4,frame.keyframe?1:0);view.setUint16(6,HEADER,true);
  view.setBigUint64(8,BigInt(`0x${streamId}`),true);view.setBigUint64(16,sequence,true);
  view.setBigUint64(24,BigInt(frame.timestampUs),true);view.setUint32(32,frame.data.length,true);view.setUint32(36,offset,true);
  view.setUint16(40,index,true);view.setUint16(42,count,true);view.setUint32(44,Math.round(1000000/frame.requestedFPS),true);
  bytes.set(frame.data.subarray(offset,offset+length),HEADER);return bytes;
}

export function createRemoteHevcSender({pc,sessionId,sendControl,requestKeyFrame=()=>{},onReady=()=>{},onFallback=()=>{},onMetrics,limits={}}) {
  const send=controlSender(sessionId,sendControl),metrics=metricsCounter("sender",onMetrics);
  let channel,closed=false,failed=false,supported=false,ready=false,busy=false,needKey=true;
  let streamId=null,configKey=null,revision=null,sequence=0n,pending=null,lastKeyRequest=-Infinity,lastMetadata=-Infinity;
  let lastNativeSequence=null,lastNativeTimestamp=null,acknowledgedSequence=0n,firstUnacknowledgedAt=0;
  let pendingDelivery=null,sendGeneration=0;
  const channelWaiters=new Set();
  const maxBuffer=integer(limits.maxBufferedAmount,1024,MAX_BUFFER)?limits.maxBufferedAmount:MAX_BUFFER;
  const configTimeout=integer(limits.configTimeoutMs,1,CONFIG_TIMEOUT)?limits.configTimeoutMs:CONFIG_TIMEOUT;
  function cancelPending() {
    if(pending){clearTimeout(pending.timer);pending.resolve(false);pending=null;}
    if(pendingDelivery){clearTimeout(pendingDelivery.timer);pendingDelivery.resolve(false);pendingDelivery=null;}
    for(const cancel of [...channelWaiters])cancel();
  }
  function fail(reason,notify=true) {
    if (closed||failed)return;failed=true;ready=false;++sendGeneration;cancelPending();clearInterval(deliveryTimer);
    const message=String(reason?.message??reason).slice(0,256);
    if(notify)send("fallback",{reason:message});try{channel?.close();}catch{}onFallback(message);
  }
  function keyRequest(reason) {
    needKey=true;
    if(now()-lastKeyRequest<200)return;
    lastKeyRequest=now();metrics.data.keyframeRequests++;
    try {Promise.resolve(requestKeyFrame(reason)).catch(error=>fail(error));} catch(error){fail(error);}
  }
  const deliveryTimer=setInterval(()=>{
    if(!closed&&!failed&&ready&&firstUnacknowledgedAt&&now()-firstUnacknowledgedAt>1000){
      firstUnacknowledgedAt=now();keyRequest("decoded-frame-ack-timeout");
    }
  },200);
  deliveryTimer.unref?.();
  try {
    channel=pc.createDataChannel(HEVC_CHANNEL,{ordered:false,maxRetransmits:0});channel.binaryType="arraybuffer";
    channel.addEventListener("close",()=>fail("HEVC 视频通道已关闭"));
    channel.addEventListener("error",()=>fail("HEVC 视频通道错误"));
  }catch(error){queueMicrotask(()=>fail(error));}
  async function waitChannel() {
    if(channel?.readyState==="open")return true;
    if(!channel||["closing","closed"].includes(channel.readyState))return false;
    return new Promise(resolve=>{
      const done=()=>{clearTimeout(timer);channelWaiters.delete(done);channel.removeEventListener("open",done);channel.removeEventListener("close",done);resolve(!closed&&!failed&&channel.readyState==="open");};
      const timer=setTimeout(done,configTimeout);channelWaiters.add(done);channel.addEventListener("open",done);channel.addEventListener("close",done);
    });
  }
  async function configure(frame,codec,nextRevision) {
    streamId=newEpoch();configKey=`${codec}/${frame.width}/${frame.height}`;revision=nextRevision;sequence=0n;ready=false;needKey=true;
    acknowledgedSequence=0n;firstUnacknowledgedAt=0;
    metrics.data.codecString=codec;metrics.data.width=frame.width;metrics.data.height=frame.height;
    const token=streamId;
    const acknowledged=new Promise(resolve=>{
      pending={resolve,token,timer:setTimeout(()=>{if(pending?.token===token){pending=null;resolve(false);}},configTimeout)};
    });
    if (!send("config",{streamId:token,revision:nextRevision,codec,codedWidth:frame.width,codedHeight:frame.height,metadata:nativeMetadata(frame)}))cancelPending();
    const accepted=await acknowledged;
    if(closed||failed||streamId!==token)return false;
    if(!accepted){fail("HEVC 解码配置未获确认");return false;}
    return true;
  }
  async function sendFrame(value,{revision:nextRevision=0}={}) {
    if(closed||failed||!supported)return false;
    if(busy){metrics.data.framesDropped++;keyRequest("sender-busy");return false;}
    busy=true;
    const generation=sendGeneration,cancelled=()=>closed||failed||generation!==sendGeneration;
    try {
      const frame=bytesOf(value)?decodeNativeHevcFrame(value):value;
      if(!frame || frame.unchanged)return false;
      if(!bytesOf(frame.data)||!frame.data.length||frame.data.length>MAX_FRAME||!codedSize(frame.width,frame.height)
          ||!integer(frame.timestampUs,0,Number.MAX_SAFE_INTEGER)||!integer(frame.requestedFPS,1,240)||!integer(nextRevision,0,Number.MAX_SAFE_INTEGER))throw new Error("HEVC 帧参数无效");
      metrics.data.framesEncoded++;Object.assign(metrics.data,nativeMetadata(frame));
      if(frame.keyframe&&!independentKey(frame.data))throw new Error("HEVC 关键帧缺少独立解码参数");
      const codec=hevcCodecFromAnnexB(frame.data)??metrics.data.codecString;
      const nativeSequence=typeof frame.sequence==="string"&&/^\d{1,20}$/.test(frame.sequence)?BigInt(frame.sequence):null;
      const restarted=(nativeSequence!==null&&lastNativeSequence!==null&&nativeSequence<=lastNativeSequence)
        ||(lastNativeTimestamp!==null&&frame.timestampUs<lastNativeTimestamp);
      lastNativeSequence=nativeSequence;lastNativeTimestamp=frame.timestampUs;
      const changed=configKey!==`${codec}/${frame.width}/${frame.height}`||revision!==nextRevision||restarted;
      if(changed){
        if(!frame.keyframe||!codecValid(codec)){metrics.data.framesDropped++;keyRequest("configuration-needs-key");return false;}
        const configured=await configure(frame,codec,nextRevision);
        if(cancelled()||!configured)return false;
      }
      if(needKey&&!frame.keyframe){metrics.data.framesDropped++;keyRequest("reference-frame-missing");return false;}
      const token=streamId;
      const channelOpen=await waitChannel();
      if(cancelled()||token!==streamId)return false;
      if(!channelOpen){fail("HEVC 视频通道未就绪");return false;}
      const size=packetSize(pc,limits.packetBytes),stride=size-HEADER;
      if(stride<=0)throw new Error("SCTP 消息尺寸不足");
      const count=Math.ceil(frame.data.length/stride);
      if(count>65535)throw new Error("HEVC 视频分片过多");
      const total=frame.data.length+count*HEADER;
      if(total>maxBuffer)throw new Error("HEVC 单帧超出有界发送缓冲");
      if(channel.bufferedAmount+total>maxBuffer){metrics.data.framesDropped++;keyRequest("network-backpressure");return false;}
      const current=++sequence;
      try {
        for(let index=0;index<count;index++){
          const packet=makePacket(streamId,current,frame,index,count,stride);channel.send(packet);
          metrics.data.bytesSent+=packet.length;metrics.data.fragmentsSent++;
        }
      }catch(error){metrics.data.framesDropped++;keyRequest("partial-frame-send");if(channel.readyState!=="open")fail(error);return false;}
      metrics.data.framesSent++;if(frame.keyframe)needKey=false;
      if(!firstUnacknowledgedAt)firstUnacknowledgedAt=now();
      if(!ready){
        // Keep the first IDR as the sole in-flight capture while a newly opened
        // hardware decoder warms up. RTP stays attached until actual output.
        const decoded=await new Promise(resolve=>{
          pendingDelivery={token,sequence:current,resolve,timer:setTimeout(()=>{
            if(pendingDelivery?.token===token){pendingDelivery=null;resolve(false);}
          },1000)};
        });
        if(cancelled()||token!==streamId)return false;
        if(!decoded){keyRequest("first-frame-decode-timeout");return false;}
        ready=true;onReady({...metrics.data});
      }
      if(now()-lastMetadata>=1000){lastMetadata=now();send("metrics",{streamId,metadata:nativeMetadata(frame)});}
      metrics.notice();return true;
    } catch(error){if(!cancelled())fail(error);return false;} finally{busy=false;}
  }
  function handleControl(value) {
    const msg=parseControl(value,sessionId);if(!msg)return false;
    if(closed||failed)return true;
    if(msg.action==="capability"){
      if(typeof msg.supported!=="boolean")return true;supported=msg.supported;
      if(!supported)fail(msg.reason??"对方不支持 WebCodecs HEVC",false);
    }else if(msg.action==="config-ack" && pending?.token===msg.streamId){
      const active=pending;pending=null;clearTimeout(active.timer);active.resolve(msg.supported===true);
    }else if(msg.action==="frame-ack"&&msg.streamId===streamId&&typeof msg.sequence==="string"&&/^\d{1,20}$/.test(msg.sequence)){
      const received=BigInt(msg.sequence);
      if(received>acknowledgedSequence&&received<=sequence){
        acknowledgedSequence=received;firstUnacknowledgedAt=received===sequence?0:now();
        if(pendingDelivery?.token===streamId&&received>=pendingDelivery.sequence){
          const active=pendingDelivery;pendingDelivery=null;clearTimeout(active.timer);active.resolve(true);
        }
      }
    }else if(msg.action==="reset"&&integer(msg.revision,0,Number.MAX_SAFE_INTEGER)){
      if(revision!==null&&msg.revision<revision)return true;
      reset();if(msg.paused!==true)keyRequest("receiver-revision-reset");
    }else if(msg.action==="request-keyframe" && msg.streamId===streamId)keyRequest("receiver-keyframe-request");
    else if(msg.action==="fallback")fail(msg.reason??"对方已回退视频传输",false);
    return true;
  }
  function reset() {++sendGeneration;cancelPending();streamId=null;configKey=null;revision=null;ready=false;needKey=true;lastKeyRequest=-Infinity;lastNativeSequence=null;lastNativeTimestamp=null;firstUnacknowledgedAt=0;}
  function close() {if(closed)return;closed=true;ready=false;++sendGeneration;cancelPending();clearInterval(deliveryTimer);try{channel?.close();}catch{/* already closed */}}
  return {get supported(){return supported;},get ready(){return ready;},get failed(){return failed;},
    sendFrame,handleControl,stats:metrics.snapshot,reset,close,fail};
}

function parsePacket(value) {
  const bytes=bytesOf(value);if(!bytes||bytes.length<=HEADER||bytes.length>=60000)return null;
  const v=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);
  if(v.getUint32(0,true)!==0x31434858||v.getUint16(6,true)!==HEADER||v.getUint8(4)>1||v.getUint8(5)!==0)return null;
  const total=v.getUint32(32,true),offset=v.getUint32(36,true),index=v.getUint16(40,true),count=v.getUint16(42,true);
  const payload=bytes.subarray(HEADER),timestampUs=safeUs(v.getBigUint64(24,true));
  if(!total||total>MAX_FRAME||!count||index>=count||timestampUs===null||offset+payload.length>total)return null;
  const stride=count===1?total:index===count-1?offset/index:payload.length;
  if(!integer(stride,1,59999-HEADER)||Math.ceil(total/stride)!==count||offset!==index*stride||payload.length!==Math.min(stride,total-offset))return null;
  return {streamId:v.getBigUint64(8,true).toString(16).padStart(16,"0"),sequence:v.getBigUint64(16,true),timestampUs,
    keyframe:!!v.getUint8(4),total,offset,index,count,stride,durationUs:v.getUint32(44,true),payload,wireBytes:bytes.length};
}

export function createRemoteHevcReceiver({sessionId,sendControl,onFrame=()=>{},onFallback=()=>{},onMetrics,
  VideoDecoder:Decoder=globalThis.VideoDecoder,EncodedVideoChunk:Chunk=globalThis.EncodedVideoChunk,limits={}}) {
  const send=controlSender(sessionId,sendControl),metrics=metricsCounter("receiver",onMetrics);
  let closed=false,failed=false,channel=null,decoder=null,configuration=null,streamId=null,configGeneration=0;
  let expectedRevision=null,configurationRevision=null,paused=false;
  let waitingKey=true,nextSequence=null,lastSequence=-1n,pendingBytes=0,lastKeyRequest=-Infinity;
  const pending=new Map(),inflight=new Map();
  const timeout=integer(limits.frameTimeoutMs,1,1000)?limits.frameTimeoutMs:FRAME_TIMEOUT;
  const sweepMs=Math.max(10,Math.min(50,timeout));
  let sweepTimer=null,draining=false,scheduledDrain=null,ackTimer=null,pendingAck=null,lastAckAt=-Infinity;
  const ensureSweep=()=>{if(!sweepTimer){sweepTimer=setInterval(sweep,sweepMs);sweepTimer.unref?.();}};
  function clearFrames(){
    pending.clear();pendingBytes=0;inflight.clear();scheduledDrain=null;
    clearTimeout(ackTimer);ackTimer=null;pendingAck=null;lastAckAt=-Infinity;
  }
  function closeDecoder(){if(decoder){try{decoder.close();}catch{/* already closed */}decoder=null;}}
  function fail(reason,notify=true) {
    if(closed||failed)return;failed=true;++configGeneration;closeDecoder();clearFrames();clearInterval(sweepTimer);sweepTimer=null;
    const message=String(reason?.message??reason).slice(0,256);if(notify)send("fallback",{reason:message});try{channel?.close();}catch{}onFallback(message);
  }
  function requestKey(reason) {
    if(!streamId||now()-lastKeyRequest<200||closed||failed)return;
    lastKeyRequest=now();metrics.data.keyframeRequests++;send("request-keyframe",{streamId,reason});
  }
  function sendFrameAck() {
    ackTimer=null;const active=pendingAck;pendingAck=null;
    if(!active||closed||failed||active.generation!==configGeneration||active.instance!==decoder||active.streamId!==streamId)return;
    lastAckAt=now();send("frame-ack",{streamId:active.streamId,sequence:active.sequence.toString()});
  }
  function acknowledgeFrame(sequence,generation,instance) {
    if(!pendingAck||sequence>pendingAck.sequence)pendingAck={sequence,generation,instance,streamId};
    const remaining=200-(now()-lastAckAt);
    if(remaining<=0){clearTimeout(ackTimer);sendFrameAck();}
    else if(!ackTimer)ackTimer=setTimeout(sendFrameAck,remaining);
  }
  function makeDecoder() {
    closeDecoder();const generation=configGeneration;
    const instance=new Decoder({output:frame=>{
      try {
        if(closed||failed||generation!==configGeneration||decoder!==instance)return;
        const entry=inflight.get(frame.timestamp);inflight.delete(frame.timestamp);
        if(!entry)return;
        metrics.data.framesDecoded++;metrics.data.width=frame.displayWidth;metrics.data.height=frame.displayHeight;
        metrics.data.decodeMs=now()-entry.submitted;metrics.data.receiveToDecodeMs=now()-entry.arrived;
        onFrame(frame,{...metrics.data,revision:configurationRevision,sequence:entry.sequence.toString(),timestampUs:frame.timestamp});
        if(closed||failed||generation!==configGeneration||decoder!==instance)return;
        acknowledgeFrame(entry.sequence,generation,instance);metrics.notice();
      }catch(error){fail(error);}finally{frame.close();scheduleDrain(generation,instance);}
    },error:error=>{if(generation===configGeneration)fail(error);}});
    decoder=instance;
    instance.addEventListener?.("dequeue",()=>scheduleDrain(generation,instance));
    instance.configure(configuration);
  }
  function loseReference(reason,count=1) {
    // Already submitted frames still have valid references. Let them finish;
    // recreating the hardware decoder here would restart its warm-up on every
    // congested burst. Only an independently decodable IRAP resumes input.
    metrics.data.framesDropped+=count;pending.clear();pendingBytes=0;waitingKey=true;nextSequence=null;
    requestKey(reason);
  }
  function sweep() {
    if(closed||failed)return;
    const at=now();
    if([...pending.values()].some(frame=>at-frame.arrived>timeout)){loseReference("fragment-timeout",Math.max(1,pending.size));return;}
    if([...inflight.values()].some(frame=>at-frame.submitted>1500)){fail("HEVC 解码输出超时");return;}
    if(waitingKey&&configuration&&at-lastKeyRequest>500)requestKey("waiting-for-keyframe");
  }
  function scheduleDrain(generation,instance) {
    if(closed||failed||generation!==configGeneration||decoder!==instance)return;
    if(scheduledDrain?.generation===generation&&scheduledDrain.instance===instance)return;
    const task={generation,instance};scheduledDrain=task;
    queueMicrotask(()=>{
      if(scheduledDrain!==task)return;
      scheduledDrain=null;
      if(!closed&&!failed&&generation===configGeneration&&decoder===instance)drain();
    });
  }
  function drain() {
    if(draining)return;
    draining=true;
    try {
    // Output can resume this path without another packet arriving. Keep the
    // existing age bound even if the document's sweep timer was delayed.
    sweep();
    while(!closed&&!failed&&decoder&&configuration) {
      // Decoder occupancy is not a lost reference. Retain at most the existing
      // three pending frames / 8 MiB until output frees a submission slot.
      if(decoder.decodeQueueSize>=MAX_DECODE||inflight.size>=MAX_DECODE)return;
      let entry;
      if(!waitingKey)entry=pending.get(nextSequence?.toString());
      if(waitingKey||!entry?.complete){
        const keys=[...pending.values()].filter(frame=>frame.complete&&frame.keyframe&&frame.sequence>lastSequence).sort((a,b)=>a.sequence<b.sequence?-1:1);
        entry=keys.at(-1);if(!entry)return;
        // An IRAP can bridge a completely missing frame, including the last
        // change on an otherwise static desktop. Do not discard that recovery
        // key merely because the missing predecessor has no partial assembly.
        for(const [id,frame]of pending)if(frame.sequence<entry.sequence){pending.delete(id);pendingBytes-=frame.total;metrics.data.framesDropped++;}
      }
      if(!entry?.complete)return;
      pending.delete(entry.sequence.toString());pendingBytes-=entry.total;
      if(entry.keyframe&&!independentKey(entry.data)){loseReference("invalid-keyframe");return;}
      if(waitingKey&&!entry.keyframe){loseReference("missing-keyframe");return;}
      const generation=configGeneration,instance=decoder;
      try {
        const submitted=now();inflight.set(entry.timestampUs,{submitted,arrived:entry.arrived,sequence:entry.sequence});
        // Commit before decode: a synchronous test decoder can call onFrame,
        // which may pause/reset/close the receiver before decode returns.
        waitingKey=false;lastSequence=entry.sequence;nextSequence=entry.sequence+1n;
        instance.decode(new Chunk({type:entry.keyframe?"key":"delta",timestamp:entry.timestampUs,duration:entry.durationUs,data:entry.data}));
        if(generation!==configGeneration||decoder!==instance)return;
      }catch(error){if(generation===configGeneration&&decoder===instance){inflight.delete(entry.timestampUs);fail(error);}return;}
    }
    }finally{draining=false;}
  }
  function receive(event) {
    if(closed||failed||!configuration)return;
    sweep();if(failed)return;
    const packet=parsePacket(event.data);
    if(!packet){metrics.data.invalidPackets++;return;}
    if(packet.streamId!==streamId||packet.sequence<=lastSequence)return;
    metrics.data.bytesReceived+=packet.wireBytes;metrics.data.fragmentsReceived++;
    if(waitingKey&&!packet.keyframe){requestKey("delta-without-reference");return;}
    const id=packet.sequence.toString();let frame=pending.get(id);
    if(!frame){
      if(pending.size>=MAX_PENDING||pendingBytes+packet.total>MAX_REASSEMBLY){loseReference("reassembly-backpressure",Math.max(1,pending.size));if(!packet.keyframe)return;}
      frame={...packet,payload:undefined,data:new Uint8Array(packet.total),parts:new Uint8Array(packet.count),received:0,arrived:now(),complete:false};
      pending.set(id,frame);pendingBytes+=packet.total;
    }
    if(frame.total!==packet.total||frame.count!==packet.count||frame.stride!==packet.stride||frame.timestampUs!==packet.timestampUs||frame.keyframe!==packet.keyframe){metrics.data.invalidPackets++;loseReference("inconsistent-fragments");return;}
    if(frame.parts[packet.index]){metrics.data.duplicatePackets++;return;}
    frame.data.set(packet.payload,packet.offset);frame.parts[packet.index]=1;frame.received++;
    if(frame.received===frame.count){frame.complete=true;metrics.data.framesReceived++;}
    drain();metrics.notice();
  }
  function decoderConfig(msg) {
    if(!codecValid(msg.codec)||!codedSize(msg.codedWidth,msg.codedHeight))return null;
    // Omitting description explicitly selects Annex B per the WebCodecs HEVC registration.
    return {codec:msg.codec,codedWidth:msg.codedWidth,codedHeight:msg.codedHeight,hardwareAcceleration:"prefer-hardware",optimizeForLatency:true};
  }
  async function probe({codec="hev1.1.6.L120.B0",codedWidth=1920,codedHeight=1080}={}) {
    let supported=false;
    try {const config=decoderConfig({codec,codedWidth,codedHeight});supported=!!(config&&Decoder&&Chunk&&(await Decoder.isConfigSupported(config)).supported);}catch{/* unsupported */}
    if(closed||failed)return false;
    send("capability",{supported,...(!supported?{reason:"当前 WebView 不支持 WebCodecs HEVC"}:{})});return supported;
  }
  async function configure(msg) {
    if(paused||!epochValid(msg.streamId)||!integer(msg.revision,0,Number.MAX_SAFE_INTEGER)
        ||(expectedRevision!==null&&msg.revision!==expectedRevision))return;
    const generation=++configGeneration,config=decoderConfig(msg);
    try {
      if(!config||!Decoder||!Chunk||!(await Decoder.isConfigSupported(config)).supported)throw new Error("当前 WebView 不支持实际 HEVC 编码配置");
      if(closed||failed||generation!==configGeneration)return;
      clearFrames();closeDecoder();streamId=msg.streamId;configuration=config;configurationRevision=msg.revision;waitingKey=true;nextSequence=null;lastSequence=-1n;lastKeyRequest=now();
      Object.assign(metrics.data,readMetadata(msg.metadata),{codecString:config.codec,width:config.codedWidth,height:config.codedHeight});
      makeDecoder();ensureSweep();
      send("config-ack",{streamId,supported:true});
    }catch(error){
      if(closed||failed||generation!==configGeneration)return;
      send("config-ack",{streamId:msg.streamId,supported:false});fail(error);
    }
  }
  function handleControl(value) {
    const msg=parseControl(value,sessionId);if(!msg)return false;if(closed||failed)return true;
    if(msg.action==="probe")void probe();
    else if(msg.action==="config")void configure(msg);
    else if(msg.action==="metrics"&&msg.streamId===streamId)Object.assign(metrics.data,readMetadata(msg.metadata));
    else if(msg.action==="fallback")fail(msg.reason??"对方已回退视频传输",false);
    return true;
  }
  function bindChannel(value) {
    if(value?.label!==HEVC_CHANNEL)return false;
    if(channel===value)return true;
    if(closed||failed||channel){try{value.close();}catch{/* stale channel */}return true;}
    if(value.ordered!==false||value.maxRetransmits!==0){fail("HEVC 通道可靠性参数无效");try{value.close();}catch{}return true;}
    channel=value;channel.binaryType="arraybuffer";channel.addEventListener("message",receive);
    channel.addEventListener("close",()=>fail("HEVC 视频通道已关闭"));channel.addEventListener("error",()=>fail("HEVC 视频通道错误"));return true;
  }
  function reset({revision,paused:nextPaused=false}={}) {
    if(closed||failed)return;
    ++configGeneration;clearFrames();closeDecoder();clearInterval(sweepTimer);sweepTimer=null;
    configuration=null;configurationRevision=null;streamId=null;waitingKey=true;nextSequence=null;lastSequence=-1n;
    if(integer(revision,0,Number.MAX_SAFE_INTEGER))expectedRevision=revision;
    paused=nextPaused===true;
    send("reset",{revision:expectedRevision??0,paused});
  }
  function close() {if(closed)return;closed=true;++configGeneration;clearInterval(sweepTimer);sweepTimer=null;clearFrames();closeDecoder();try{channel?.close();}catch{/* already closed */}}
  return {get failed(){return failed;},get ready(){return !!configuration&&!paused&&!closed&&!failed;},probe,handleControl,bindChannel,stats:metrics.snapshot,reset,close};
}
