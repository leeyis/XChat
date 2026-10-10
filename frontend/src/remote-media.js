import {remoteAccepted,remoteInputAllowed,remoteQuality} from "./remote-model.js";
import {createNativeFrameReader,decodeNativeFrame,presentNativeFrame,preferRemoteVideoCodecs} from "./remote-capture.js";
import {remoteMediaMetrics} from "./remote-metrics.js";
import {readRemoteTelemetry,remoteTelemetry} from "./remote-telemetry.js";
import {watchRemoteConnection} from "./remote-connection.js";
import {attachRemoteVoice,syncRemoteVoice} from "./remote-voice.js";
import {RemoteHevcMedia} from "./remote-hevc-media.js";

const stop=stream=>stream?.getTracks().forEach(track=>track.stop());
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));

export class RemoteMedia {
  constructor({session,native,browserStream,signal,frame,openCaptureStream,input,changed,failed,voiceFailed,localInfo}) {
    Object.assign(this,{session,native,signal,frame,openCaptureStream,input,changed,failed,voiceFailed,localInfo});
    this.closed=false;this.audioEpoch=0;this.inputSequence=0;this.inputQueue=[];this.inputWorking=false;this.heldInputs=new Set();
    this.localScreen=browserStream||null;this.remoteScreen=new MediaStream();this.microphone=null;
    this.speaker=true;this.audio=new Audio();this.audio.autoplay=true;this.audio.playsInline=true;
    this.pc=new RTCPeerConnection({iceServers:[],bundlePolicy:"max-bundle"});
    this.rtpScreen=this.remoteScreen;this.hevc=new RemoteHevcMedia(this);
    this.pc.ontrack=event=>{
      if(this.closed)return;
      if(event.track.kind==="video"){
        this.rtpScreen.getTracks().forEach(track=>this.rtpScreen.removeTrack(track));
        this.rtpScreen.addTrack(event.track);
        if(!this.hevc.receiving)this.remoteScreen=this.rtpScreen;
        this.changed();
      } else {this.audio.srcObject=new MediaStream([event.track]);void this.playAudio();}
    };
    this.pc.ondatachannel=event=>this.bindChannel(event.channel);
    this.connection=watchRemoteConnection(this);
    this.statsTimer=setInterval(()=>void this.stats(),2000);
    this.inputKeepAlive=setInterval(()=>{if(this.holdingInput)this.sendInput({type:"keep_alive"});},350);
    this.ready=this.initialize().catch(error=>this.fail(error));
  }
  async initialize() {
    if(this.session.local_host){
      if(this.native){
        this.frameReader=createNativeFrameReader({frame:this.frame,sessionId:this.session.id});
        this.canvas=document.createElement("canvas");this.canvas.width=1280;this.canvas.height=720;
        this.localScreen=this.canvas.captureStream(0);
        this.localScreen.getVideoTracks()[0].contentHint="detail";
        this.changed();void this.captureLoop();
      } else if(!this.localScreen?.getVideoTracks().some(t=>t.readyState==="live"))throw new Error("共享屏幕已经关闭，请重新选择");
      const source=this.localScreen;
      source.getVideoTracks().forEach(track=>track.addEventListener("ended",()=>{if(!this.closed&&this.localScreen===source)this.fail(new Error("屏幕共享已停止"));},{once:true}));
    }
    if(this.closed)return;
    if(this.session.initiator){
      this.video=this.pc.addTransceiver("video",{direction:"sendrecv"});
      preferRemoteVideoCodecs(this.video);
      this.voice=this.pc.addTransceiver("audio",{direction:"sendrecv"});
      this.bindChannel(this.pc.createDataChannel("xchat-control",{ordered:true}));
      await this.attachTracks();await this.pc.setLocalDescription(await this.pc.createOffer());
      await this.gather();if(this.closed)return;
      await this.signal({type:"description",kind:this.pc.localDescription.type,sdp:this.pc.localDescription.sdp});
    }
    void this.syncVoice();this.changed();
  }
  async gather() {
    const deadline=Date.now()+4500;
    while(!this.closed&&this.pc.iceGatheringState!=="complete"&&Date.now()<deadline)await delay(40);
    // A bounded wait includes currently collected LAN candidates in one ordered description.
  }
  async handle(body) {
    await this.ready;if(this.closed)return;
    if(body.type==="description"){
      if(body.kind==="offer"){
        if(this.session.initiator||this.pc.signalingState!=="stable")throw new Error("远程会话描述顺序错误");
        await this.pc.setRemoteDescription({type:"offer",sdp:body.sdp});
        this.video=this.pc.getTransceivers().find(t=>t.receiver.track.kind==="video");
        preferRemoteVideoCodecs(this.video);
        this.voice=this.pc.getTransceivers().find(t=>t.receiver.track.kind==="audio");
        if(!this.video||!this.voice)throw new Error("对方未提供完整的屏幕/语音通道");
        this.video.direction="sendrecv";this.voice.direction="sendrecv";
        await this.attachTracks();await this.pc.setLocalDescription(await this.pc.createAnswer());await this.gather();
        if(!this.closed)await this.signal({type:"description",kind:"answer",sdp:this.pc.localDescription.sdp});
      }else if(body.kind==="answer"&&this.session.initiator&&this.pc.signalingState==="have-local-offer")await this.pc.setRemoteDescription({type:"answer",sdp:body.sdp});
    }else if(body.type==="candidate")await this.pc.addIceCandidate(body.candidate);
    void this.applyQuality();
  }
  async attachTracks() {
    if(this.closed)return;
    await this.video?.sender.replaceTrack(this.session.local_host?this.localScreen?.getVideoTracks()[0]||null:null);
    await attachRemoteVoice(this);
  }
  update(session) {
    const prior=this.session;this.session=session;
    if(prior.grant!==session.grant){this.inputSuspended=false;this.inputSequence=0;this.inputQueue=[];this.holdingInput=false;this.heldInputs.clear();}
    if(!remoteAccepted(session)){this.close();return;}
    this.hevc.update(session,prior);
    if(session.local_host){
      this.localScreen?.getVideoTracks().forEach(track=>track.enabled=!session.paused);
      if(this.native&&prior.screen?.id!==session.screen?.id){this.clearCanvas();}
    }
    if(prior.paused!==session.paused&&session.paused)this.clearCanvas();
    if(prior.voice.stage!==session.voice.stage||prior.voice.id!==session.voice.id)void this.syncVoice();
    this.microphone?.getAudioTracks().forEach(track=>track.enabled=!session.voice.local_muted);
    if(JSON.stringify(prior.quality)!==JSON.stringify(session.quality))void this.applyQuality();
  }
  clearCanvas(){if(this.canvas){const ctx=this.canvas.getContext("2d");ctx.fillStyle="#202b30";ctx.fillRect(0,0,this.canvas.width,this.canvas.height);this.localScreen?.getVideoTracks()[0]?.requestFrame?.();}}
  async captureLoop() {
    while(!this.closed){
      const began=performance.now(),session=this.session;
      if(!session.paused){
        try {
          if(await this.hevc.capture(session.revision)){this.captureBusySince=null;continue;}
          if(this.closed)break;
          const received=await this.frameReader.read(session.revision);
          this.captureBusySince=null;
          try{
            if(this.closed)break;
            if(this.session.revision!==session.revision)continue;
            const frame=decodeNativeFrame(received.bytes),presented=performance.now();
            if(presentNativeFrame(this.canvas,this.localScreen.getVideoTracks()[0],frame)){
              this.captureMetrics={captureBackend:frame.captureBackend,captureMs:frame.captureMs,bridgeMs:frame.nativeMs===null?null:Math.max(0,presented-began-frame.nativeMs),canvasMs:performance.now()-presented,sourceWidth:frame.sourceWidth,sourceHeight:frame.sourceHeight,frameTransport:received.transport};
            }
          }finally{received.release();}
        }catch(error){
          if(/remote_capture_busy|正在处理上一帧/.test(String(error))&&!this.closed){
            this.captureBusySince??=performance.now();
            if(performance.now()-this.captureBusySince<5000){await delay(25);continue;}
          }
          if(!this.closed&&!this.session.paused&&session.revision===this.session.revision&&!String(error).includes("remote_state_changed")){this.fail(error);break;}
          if(!this.closed)await delay(50);
        }
      }
      // Native requests are paced on their dedicated Rust worker. setTimeout
      // here would reduce an occluded/minimized WebView to roughly 1 fps.
      if(session.paused||!this.native)await delay(Math.max(8,1000/remoteQuality(session.quality).maxFramerate-(performance.now()-began)));
    }
  }
  async changeBrowserScreen(stream) {
    const old=this.localScreen;this.localScreen=stream;
    await this.video?.sender.replaceTrack(stream.getVideoTracks()[0]);stop(old);
    stream.getVideoTracks()[0].addEventListener("ended",()=>{if(!this.closed&&this.localScreen===stream)this.fail(new Error("屏幕共享已停止"));},{once:true});this.changed();
  }
  syncVoice(deviceId=this.inputDevice) {return syncRemoteVoice(this,deviceId);}
  async playAudio() {
    try{await this.audio.play();this.audioBlocked=false;}catch{this.audioBlocked=true;}this.changed();
  }
  setSpeaker(enabled){this.speaker=enabled;this.audio.muted=!enabled;if(enabled)void this.playAudio();this.changed();}
  async outputDevice(id){if(typeof this.audio.setSinkId!=="function")throw new Error("当前环境由系统选择扬声器");await this.audio.setSinkId(id);this.outputId=id;}
  async applyQuality(){
    const sender=this.video?.sender;if(!sender||this.closed||!this.session.local_host)return;
    try{const parameters=sender.getParameters();if(!parameters.encodings?.length)return;
      Object.assign(parameters.encodings[0],remoteQuality(this.session.quality));
      parameters.degradationPreference="maintain-resolution";
      await sender.setParameters(parameters);
      const track=this.localScreen?.getVideoTracks()[0];if(track&&!this.native)await track.applyConstraints({frameRate:{max:remoteQuality(this.session.quality).maxFramerate}});
    }catch{/* Browser congestion control remains active when a codec cannot apply a hint. */}
  }
  bindChannel(channel){
    if(this.hevc?.bindChannel(channel))return;
    if(channel.label!=="xchat-control"){channel.close();return;}
    if(this.channel&&this.channel!==channel&&this.channel.readyState!=="closed"){channel.close();return;}this.channel=channel;
    channel.onmessage=event=>{
      if(this.closed||typeof event.data!=="string"||event.data.length>4096)return;
      if(this.hevc?.handleControl(event.data))return;
      if(event.data.length>2048)return;
      if(!this.session.local_host){
        const info=readRemoteTelemetry(event.data);
        if(info){this.hostMetrics=info;this.metrics={...this.metrics,...info};this.changed();}
        return;
      }
      if(!this.native||this.session.paused||this.inputSuspended)return;
      try{const packet=JSON.parse(event.data);if(packet.grant!==this.session.grant||!packet.grant)return;
        if(this.inputQueue.length>=64)throw new Error("远程输入积压，已收回控制");
        this.inputQueue.push(packet);void this.drainInput();
      }catch(error){void this.revokeInput(error);}
    };
    this.connection.bind(channel);
    channel.addEventListener?.("open",()=>this.hevc?.controlOpen());
    if(channel.readyState==="open")this.hevc?.controlOpen();
  }
  async revokeInput(error){this.inputQueue=[];this.changed({inputError:error.message});await this.signal({type:"control",allow:false}).catch(()=>{});}
  async drainInput(){
    if(this.inputWorking)return;this.inputWorking=true;
    try{while(!this.closed&&this.inputQueue.length){const packet=this.inputQueue.shift();if(packet.grant!==this.session.grant)continue;try{await this.input(packet);}catch(error){if(!this.closed&&packet.grant===this.session.grant)await this.revokeInput(error);}}}
    finally{this.inputWorking=false;}
  }
  sendInput(event){
    if(this.inputSuspended||!remoteInputAllowed(this.session)||this.channel?.readyState!=="open")return false;
    if(this.channel.bufferedAmount>65536)return false;
    if(event.type==="release")this.heldInputs.clear();
    else if(event.type==="key"||event.type==="button"){const key=event.type+(event.code??event.button);if(event.down)this.heldInputs.add(key);else this.heldInputs.delete(key);}
    this.holdingInput=this.heldInputs.size>0;
    this.channel.send(JSON.stringify({grant:this.session.grant,sequence:++this.inputSequence,event}));return true;
  }
  async stats(){
    if(this.closed)return;
    try{const reports=await this.pc.getStats();if(this.closed)return;
      const {info,sample}=remoteMediaMetrics(reports,this.session.local_host,this.previousStats);
      this.previousStats=sample;this.metrics={...info,...this.captureMetrics,...this.hostMetrics,...this.hevc?.stats()};
      if(this.session.local_host&&this.channel?.readyState==="open"&&this.channel.bufferedAmount<16384)this.channel.send(remoteTelemetry(this.metrics,this.localInfo));
      this.changed();
    }catch{}
  }
  fail(error){if(this.closed)return;this.failed(error);this.close();}
  close(){
    if(this.closed)return;this.closed=true;this.audioEpoch++;clearInterval(this.statsTimer);clearInterval(this.inputKeepAlive);
    this.frameReader?.close();
    this.hevc?.close();
    this.connection?.dispose();this.pc.onconnectionstatechange=null;if(this.channel)this.channel.onclose=null;
    this.channel?.close();this.pc.close();stop(this.localScreen);stop(this.microphone);stop(this.remoteScreen);
    if(this.rtpScreen!==this.remoteScreen)stop(this.rtpScreen);
    this.audio.pause();this.audio.srcObject=null;this.inputQueue=[];this.changed();
  }
}
