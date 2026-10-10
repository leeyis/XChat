// Native-local binary bridge. Pixels never enter the signaling channel and are
// encoded only once, by the negotiated WebRTC video sender.
export function decodeNativeFrame(value) {
  const bytes=value instanceof Uint8Array?value:new Uint8Array(value);
  if(bytes.byteLength<40||String.fromCharCode(...bytes.subarray(0,4))!=="XRF1")throw new Error("屏幕帧格式不受支持");
  const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength),header=view.getUint16(4,true);
  const width=view.getUint32(8,true),height=view.getUint32(12,true),unchanged=(view.getUint32(36,true)&1)!==0;
  if(![40,44].includes(header)||bytes[6]!==1||!width||!height||width>32768||height>32768||bytes.byteLength!==header+(unchanged?0:width*height*4))throw new Error("屏幕帧数据不完整");
  return {width,height,unchanged,sequence:view.getBigUint64(16,true).toString(),
    captureMs:view.getUint32(24,true)/1000,nativeMs:header>=44?view.getUint32(40,true)/1000:null,
    sourceWidth:view.getUint32(28,true),sourceHeight:view.getUint32(32,true),
    captureBackend:bytes[7]===1?"DXGI":bytes[7]===2?"GDI":"unknown",
    pixels:unchanged?null:new Uint8ClampedArray(bytes.buffer,bytes.byteOffset+header,width*height*4)};
}

export function presentNativeFrame(canvas,track,frame) {
  if(frame.unchanged)return false;
  if(canvas.width!==frame.width||canvas.height!==frame.height){canvas.width=frame.width;canvas.height=frame.height;}
  const context=canvas.getContext("2d",{alpha:false,desynchronized:true});
  context.putImageData(new ImageData(frame.pixels,frame.width,frame.height),0,0);
  track?.requestFrame?.();
  return true;
}

const sharedDispatchers=new WeakMap();
function sharedDispatcher(webview) {
  let pending=sharedDispatchers.get(webview);
  if(pending)return pending;
  pending=new Map();sharedDispatchers.set(webview,pending);
  // Keep one dispatcher for the page lifetime. A request can time out or close
  // before its event arrives; unmatched late mappings must still be released.
  webview.addEventListener("sharedbufferreceived",event=>{
    const metadata=event.additionalData;
    if(metadata?.type!=="xchat-frame-v1")return;
    let bytes;
    try{bytes=event.getBuffer();}catch(error){pending.get(metadata.requestId)?.reject(error);return;}
    let released=false;
    const release=()=>{if(!released){released=true;webview.releaseBuffer(bytes);}};
    const request=pending.get(metadata.requestId);
    if(!request||request.received||metadata.sessionId!==request.sessionId||metadata.revision!==String(request.revision)){release();return;}
    try{
      const frame=decodeNativeFrame(bytes);
      if(frame.unchanged||frame.sequence!==metadata.sequence)throw new Error("共享屏幕帧标识不匹配");
      request.received={bytes,release,sequence:frame.sequence,transport:"shared-memory"};
      request.resolve(request.received);
    }catch(error){release();request.reject(error);}
  });
  return pending;
}

export function createNativeFrameReader({frame,sessionId,webview=globalThis.chrome?.webview,randomId=()=>globalThis.crypto.randomUUID(),timeoutMs=5000}) {
  let shared=typeof webview?.addEventListener==="function"&&typeof webview?.releaseBuffer==="function"&&typeof globalThis.crypto?.randomUUID==="function";
  const pending=shared?sharedDispatcher(webview):null;
  let closed=false,active=null;
  return {
    async read(revision) {
      if(closed)throw new Error("屏幕采集已关闭");
      if(active)throw new Error("正在处理上一帧");
      if(!shared)return {bytes:await frame(revision,"rgba-v1"),release:()=>{},transport:"ipc"};
      const requestId=randomId();
      let timer,returned=false;
      const request={sessionId,revision,received:null};
      const ready=new Promise((resolve,reject)=>{request.resolve=resolve;request.reject=reject;});
      // An event/close can precede the invoke response. Attach a rejection handler
      // immediately, then propagate the same rejection from the awaited race.
      ready.catch(()=>{});
      const cancelled=new Promise((_,reject)=>{request.cancel=reject;});
      active=request;pending.set(requestId,request);
      timer=setTimeout(()=>request.cancel(new Error("共享屏幕帧传递超时")),timeoutMs);
      try{
        const value=await Promise.race([frame(revision,"rgba-shared-v1",requestId),cancelled]);
        const bytes=value instanceof Uint8Array?value:new Uint8Array(value);
        if(bytes.byteLength===12&&String.fromCharCode(...bytes.subarray(0,4))==="XRS1"){
          const sequence=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength).getBigUint64(4,true).toString();
          const received=await Promise.race([ready,cancelled]);
          if(received.sequence!==sequence)throw new Error("共享屏幕帧序号不匹配");
          returned=true;return received;
        }
        // Native returns a complete raw frame only when this public API is not
        // available. Permission/session failures reject and never take this path.
        const fallback=decodeNativeFrame(bytes);
        if(!fallback.unchanged)shared=false;
        return {bytes,release:()=>{},transport:"ipc"};
      }finally{
        clearTimeout(timer);pending.delete(requestId);active=null;
        if(!returned)request.received?.release();
      }
    },
    close(){closed=true;active?.cancel(new Error("屏幕采集已关闭"));},
  };
}

export function preferRemoteVideoCodecs(transceiver,receiver=globalThis.RTCRtpReceiver) {
  if(typeof transceiver?.setCodecPreferences!=="function")return;
  const receiving=receiver?.getCapabilities?.("video")?.codecs||[];
  // Receiver capabilities describe what this peer prefers to receive. Let the
  // browser match asymmetric H265/H264 levels against its encoder capabilities.
  // Do not intersect entire fmtp strings or append encoder variants: duplicate
  // profiles with different levels can make Chromium reject the answer's video.
  const key=codec=>JSON.stringify([codec.mimeType.toLowerCase(),codec.clockRate,codec.channels,codec.sdpFmtpLine]);
  const available=new Set();
  const codecs=receiving.filter(codec=>{
    const id=key(codec);if(available.has(id))return false;available.add(id);return true;
  });
  const order=["video/h265","video/h264","video/vp9","video/vp8","video/av1"];
  const rank=codec=>{const index=order.indexOf(codec.mimeType.toLowerCase());return index<0?order.length:index;};
  // Keep real capability objects, distinct profiles/levels and RTX/FEC. Preference is not a
  // claim that HEVC or hardware acceleration was actually negotiated.
  if(codecs.length)try{transceiver.setCodecPreferences(codecs.slice().sort((a,b)=>rank(a)-rank(b)));}catch{}
}
