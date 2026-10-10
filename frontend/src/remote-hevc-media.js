import {HEVC_CHANNEL,createRemoteHevcReceiver,decodeNativeHevcFrame} from './remote-hevc.js';
import {createRemoteHevcWorkerSender} from './remote-hevc-worker-client.js';

// The reliable input/control channel can be replaced while the independent
// HEVC video channel stays healthy. Retain only bounded HEVC protocol messages
// across that gap; accepting a queued config never replaces its actual ACK.
export function createHevcControlOutbox(media) {
  const pending=new Map(),encoder=new TextEncoder();
  let closed=false,bytes=0,bound=null,onLow=null;
  const clear=()=>{pending.clear();bytes=0;};
  function flush(){
    const channel=media.channel;
    if(closed||media.closed||channel?.readyState!=='open')return;
    for(const [key,item] of pending){
      if(channel!==media.channel||channel.readyState!=='open'||channel.bufferedAmount>16384)return;
      try{channel.send(item.json);}catch{return;}
      pending.delete(key);bytes-=item.bytes;
    }
  }
  function send(message){
    if(closed||media.closed||message?.type!=='xchat-hevc'||message.version!==1||message.sessionId!==media.session.id)return false;
    let json,length;
    try{json=JSON.stringify(message);length=encoder.encode(json).length;}catch{return false;}
    if(length>4096)return false;
    if(['config','reset','fallback'].includes(message.action))clear();
    const key=`${message.action}/${message.streamId??''}`,previous=pending.get(key);
    if(message.action==='frame-ack'&&previous&&/^\d{1,20}$/.test(message.sequence)&&/^\d{1,20}$/.test(previous.message.sequence)
        &&BigInt(message.sequence)<BigInt(previous.message.sequence))return true;
    const size=bytes-(previous?.bytes||0)+length;
    if((!previous&&pending.size>=8)||size>16384)return false;
    pending.set(key,{message,json,bytes:length});bytes=size;flush();return true;
  }
  function bind(channel){
    if(closed||channel===bound)return;
    bound?.removeEventListener?.('bufferedamountlow',onLow);
    bound=channel;channel.bufferedAmountLowThreshold=8192;
    onLow=()=>{if(media.channel===channel)flush();};
    channel.addEventListener?.('bufferedamountlow',onLow);
  }
  function close(){closed=true;clear();bound?.removeEventListener?.('bufferedamountlow',onLow);bound=null;}
  return {send,flush,bind,reset:clear,close};
}

// Native HEVC is negotiated inside the existing authenticated peer connection.
// Only the receiver owns a presentation canvas; the sharing window has no preview.
export class RemoteHevcMedia {
  constructor(media) {
    this.media=media;this.closed=false;this.receiving=false;this.keyframe=true;this.detached=false;
    this.wantDetached=false;this.trackSync=Promise.resolve();
    this.controls=createHevcControlOutbox(media);
    const sendControl=message=>this.controls.send(message);
    if(media.native&&media.session.local_host){
      this.sender=createRemoteHevcWorkerSender({pc:media.pc,sessionId:media.session.id,sendControl,
        openStream:media.openCaptureStream,
        requestKeyFrame:()=>{this.keyframe=true;},
        onReady:()=>{void this.detachRtp();},
        onFallback:reason=>{this.fallbackReason=String(reason??'').slice(0,256);void this.restoreRtp();},
      });
    }else if(!media.session.local_host){
      this.receiver=createRemoteHevcReceiver({sessionId:media.session.id,sendControl,
        onFrame:(frame,metadata)=>this.present(frame,metadata),
        onFallback:reason=>{this.fallbackReason=String(reason??'').slice(0,256);this.restoreReceiver();},
      });
    }
    this.update(media.session);
  }
  controlOpen(){
    if(this.closed)return;
    this.controls?.flush();
    if(this.receiver)void this.receiver.probe().catch(()=>{});
  }
  handleControl(data){return this.sender?.handleControl(data)||this.receiver?.handleControl(data)||false;}
  bindChannel(channel){
    if(channel.label==='xchat-control'){this.controls?.bind(channel);return false;}
    if(channel.label!==HEVC_CHANNEL)return false;
    if(this.receiver&&!this.closed)this.receiver.bindChannel(channel);else channel.close();
    return true;
  }
  syncRtp(){
    // Serial replacement prevents a late HEVC detach from undoing fallback.
    const sync=async()=>{
      if(this.closed||this.media.closed)return;
      const detached=this.wantDetached&&this.sender?.ready&&!this.sender.failed;
      await this.media.video?.sender.replaceTrack(detached?null:this.media.localScreen?.getVideoTracks()[0]||null);
      this.detached=!!detached;this.media.changed();
    };
    this.trackSync=this.trackSync.catch(()=>{}).then(sync).catch(()=>{});
    return this.trackSync;
  }
  detachRtp(){
    if(this.closed||!this.sender?.ready)return Promise.resolve();
    this.wantDetached=true;return this.syncRtp();
  }
  restoreRtp(){
    this.wantDetached=false;return this.syncRtp();
  }
  update(session,prior){
    if(this.closed||prior&&(prior.revision===session.revision&&prior.paused===session.paused))return;
    this.controls?.reset();
    this.keyframe=true;
    if(this.sender?.update)this.sender.update({revision:session.revision,paused:session.paused});
    else this.sender?.reset();
    this.receiver?.reset({revision:session.revision,paused:session.paused});
    if(this.canvas){
      const context=this.canvas.getContext('2d');context.fillStyle='#202b30';
      context.fillRect(0,0,this.canvas.width,this.canvas.height);
    }
  }
  async capture(revision){
    if(this.closed||!this.sender?.supported||this.sender.failed)return false;
    const media=this.media,sender=this.sender,began=performance.now(),keyframe=this.keyframe;
    if(sender.ownsCapture){
      const ownsCapture=await sender.capture(revision);
      if(this.closed||media.closed||media.session.revision!==revision||media.session.paused)return true;
      if(!ownsCapture||sender.failed){
        if(this.detached||this.wantDetached)await this.restoreRtp();
        return false;
      }
      return true;
    }
    this.keyframe=false;
    try{
      const bytes=await media.frame(revision,'hevc-v1',undefined,keyframe);
      if(this.closed||media.closed||this.sender!==sender||media.session.revision!==revision||media.session.paused)return true;
      const frame=decodeNativeHevcFrame(bytes),received=performance.now();
      if(frame.unchanged)return true;
      media.captureMetrics={captureBackend:frame.captureBackend,captureMs:frame.captureMs,
        bridgeMs:frame.nativeMs==null?null:Math.max(0,received-began-frame.nativeMs),
        encodeMs:frame.encodeMs,canvasMs:null,sourceWidth:frame.sourceWidth,sourceHeight:frame.sourceHeight,
        encoderImplementation:frame.encoderImplementation,hardwareEncoder:frame.hardwareEncoder,
        frameTransport:'encoded-ipc'};
      await sender.sendFrame(frame,{revision});
      return !sender.failed;
    }catch(error){
      this.keyframe=true;
      if(this.closed||media.closed)return true;
      if(media.session.revision!==revision||media.session.paused||String(error).includes('remote_state_changed'))return true;
      if(/remote_capture_busy|正在处理上一帧/.test(String(error)))throw error;
      // An unsupported encoder/decoder must leave screen sharing available.
      this.fallbackReason=String(error?.message??error).slice(0,256);
      sender.close();if(this.sender===sender)this.sender=null;
      await this.restoreRtp();
      return false;
    }
  }
  present(frame,metadata){
    if(this.closed||this.media.closed||this.media.session.paused||metadata.revision!==this.media.session.revision)return;
    if(!this.canvas)this.canvas=document.createElement('canvas');
    const width=frame.displayWidth||frame.codedWidth,height=frame.displayHeight||frame.codedHeight;
    if(this.canvas.width!==width||this.canvas.height!==height){this.canvas.width=width;this.canvas.height=height;}
    this.canvas.getContext('2d',{alpha:false,desynchronized:true}).drawImage(frame,0,0,width,height);
    if(!this.receiving){this.receiving=true;this.media.changed();}
  }
  attachSurface(canvas){
    if(this.closed)return()=>{};
    const previous=this.canvas;
    if(previous&&previous!==canvas){
      canvas.width=previous.width;canvas.height=previous.height;
      canvas.getContext('2d',{alpha:false,desynchronized:true}).drawImage(previous,0,0);
    }
    this.canvas=canvas;
    return()=>{
      if(this.canvas!==canvas)return;
      if(this.closed){this.canvas=null;return;}
      const retained=document.createElement('canvas');retained.width=canvas.width;retained.height=canvas.height;
      retained.getContext('2d',{alpha:false,desynchronized:true}).drawImage(canvas,0,0);
      this.canvas=retained;
    };
  }
  restoreReceiver(){
    if(!this.receiving||this.closed)return;
    this.receiving=false;this.media.remoteScreen=this.media.rtpScreen;
    this.media.changed();
  }
  stats(){
    const active=this.sender?.ready||this.receiving;
    const stats=this.sender?.stats()||this.receiver?.stats();
    const diagnostic={...(stats?.hevcWorker?{hevcWorker:stats.hevcWorker}:{}),
      ...(this.fallbackReason?{hevcFallbackReason:this.fallbackReason}:{})};
    // Pending/failed upgrades must be observable even before the first actual
    // HEVC frame. Do not publish HEVC codec/FPS labels for that inactive path.
    if(!active)return Object.keys(diagnostic).length?diagnostic:null;
    const host=this.receiving?{hostCaptureBackend:'DXGI',hostCaptureMs:stats.captureMs,
      hostEncodeMs:stats.encodeMs,hostEncoderImplementation:stats.encoderImplementation,
      hostHardwareEncoder:stats.hardwareEncoder,decoderImplementation:'WebCodecs HEVC',
      processingMs:stats.receiveToDecodeMs}:{};
    return {...stats,...host,...diagnostic,codec:'H265',codecParameters:stats.codecString,mediaTransport:'dtls-sctp',
      ...(stats.captureTransport?{frameTransport:stats.captureTransport}:{}),
      srtpCipher:null,powerEfficientEncoder:null,powerEfficientDecoder:null,lossPercent:null,
      jitterBufferMs:null,qualityLimitationReason:null};
  }
  close(){
    if(this.closed)return;this.closed=true;this.sender?.close();this.receiver?.close();
    this.controls?.close();
    this.canvas=null;
  }
}
