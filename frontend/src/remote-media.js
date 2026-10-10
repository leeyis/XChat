import {remoteAccepted,remoteInputAllowed,remoteQuality} from "./remote-model.js";

const stop=stream=>stream?.getTracks().forEach(track=>track.stop());
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));

export class RemoteMedia {
  constructor({session,native,browserStream,signal,frame,input,changed,failed,voiceFailed}) {
    Object.assign(this,{session,native,signal,frame,input,changed,failed,voiceFailed});
    this.closed=false;this.audioEpoch=0;this.inputSequence=0;this.inputQueue=[];this.inputWorking=false;this.heldInputs=new Set();
    this.localScreen=browserStream||null;this.remoteScreen=new MediaStream();this.microphone=null;
    this.speaker=true;this.audio=new Audio();this.audio.autoplay=true;this.audio.playsInline=true;
    this.pc=new RTCPeerConnection({iceServers:[],bundlePolicy:"max-bundle"});
    this.pc.ontrack=event=>{
      if(this.closed)return;
      if(event.track.kind==="video"){
        this.remoteScreen.getTracks().forEach(track=>this.remoteScreen.removeTrack(track));
        this.remoteScreen.addTrack(event.track);this.changed();
      } else {this.audio.srcObject=new MediaStream([event.track]);void this.playAudio();}
    };
    this.pc.ondatachannel=event=>this.bindChannel(event.channel);
    this.pc.onconnectionstatechange=()=>{
      if(this.closed)return;
      if(this.pc.connectionState==="connected")void this.signal({type:"ready"}).catch(e=>this.fail(e));
      if(["failed","closed","disconnected"].includes(this.pc.connectionState))this.fail(new Error("远程连接已中断，请重新发起协助"));
      this.changed();
    };
    this.statsTimer=setInterval(()=>void this.stats(),2000);
    this.inputKeepAlive=setInterval(()=>{if(this.holdingInput)this.sendInput({type:"keep_alive"});},350);
    this.ready=this.initialize().catch(error=>this.fail(error));
  }
  async initialize() {
    if(this.session.local_host){
      if(this.native){
        this.canvas=document.createElement("canvas");this.canvas.width=1280;this.canvas.height=720;
        this.localScreen=this.canvas.captureStream(0);
        this.changed();void this.captureLoop();
      } else if(!this.localScreen?.getVideoTracks().some(t=>t.readyState==="live"))throw new Error("共享屏幕已经关闭，请重新选择");
      const source=this.localScreen;
      source.getVideoTracks().forEach(track=>track.addEventListener("ended",()=>{if(!this.closed&&this.localScreen===source)this.fail(new Error("屏幕共享已停止"));},{once:true}));
    }
    if(this.closed)return;
    if(this.session.initiator){
      this.video=this.pc.addTransceiver("video",{direction:"sendrecv"});
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
    await this.voice?.sender.replaceTrack(this.microphone?.getAudioTracks()[0]||null);
  }
  update(session) {
    const prior=this.session;this.session=session;
    if(prior.grant!==session.grant){this.inputSequence=0;this.inputQueue=[];this.holdingInput=false;this.heldInputs.clear();}
    if(!remoteAccepted(session)){this.close();return;}
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
          const bytes=await this.frame(session.revision);if(this.closed)break;
          if(this.session.revision!==session.revision)continue;
          const bitmap=await createImageBitmap(new Blob([bytes],{type:"image/jpeg"}));
          if(this.closed||this.session.revision!==session.revision){bitmap.close();continue;}
          if(this.canvas.width!==bitmap.width||this.canvas.height!==bitmap.height){this.canvas.width=bitmap.width;this.canvas.height=bitmap.height;}
          this.canvas.getContext("2d").drawImage(bitmap,0,0);bitmap.close();this.localScreen.getVideoTracks()[0]?.requestFrame?.();
        }catch(error){if(!this.closed&&!this.session.paused&&session.revision===this.session.revision&&!String(error).includes("remote_state_changed")){this.fail(error);break;}}
      }
      await delay(Math.max(8,1000/remoteQuality(session.quality).maxFramerate-(performance.now()-began)));
    }
  }
  async changeBrowserScreen(stream) {
    const old=this.localScreen;this.localScreen=stream;
    await this.video?.sender.replaceTrack(stream.getVideoTracks()[0]);stop(old);
    stream.getVideoTracks()[0].addEventListener("ended",()=>{if(!this.closed&&this.localScreen===stream)this.fail(new Error("屏幕共享已停止"));},{once:true});this.changed();
  }
  async syncVoice(deviceId=this.inputDevice) {
    const epoch=++this.audioEpoch,call=this.session.voice.id;
    if(this.session.voice.stage!=="active"){
      const previous=this.microphone;this.microphone=null;stop(previous);this.changed();
      await this.voice?.sender.replaceTrack(null).catch(()=>{});return;
    }
    if(this.microphone&&deviceId===this.inputDevice)return;
    try {
      const stream=await navigator.mediaDevices.getUserMedia({audio:{echoCancellation:true,noiseSuppression:true,autoGainControl:true,...(deviceId?{deviceId:{exact:deviceId}}:{})},video:false});
      if(this.closed||epoch!==this.audioEpoch||this.session.voice.stage!=="active"||this.session.voice.id!==call){stop(stream);return;}
      stream.getAudioTracks().forEach(track=>track.enabled=!this.session.voice.local_muted);
      await this.voice?.sender.replaceTrack(stream.getAudioTracks()[0]);
      if(this.closed||epoch!==this.audioEpoch){stop(stream);return;}
      const old=this.microphone;this.microphone=stream;this.inputDevice=deviceId;stop(old);
      stream.getAudioTracks()[0].addEventListener("ended",()=>{if(!this.closed&&this.microphone===stream)void this.voiceFailed(new Error("麦克风已断开，可重新发起语音"));},{once:true});
      this.changed();
    }catch(error){if(!this.closed&&epoch===this.audioEpoch)await this.voiceFailed(new Error(`麦克风不可用：${error.message}`));}
  }
  async playAudio() {
    try{await this.audio.play();this.audioBlocked=false;}catch{this.audioBlocked=true;}this.changed();
  }
  setSpeaker(enabled){this.speaker=enabled;this.audio.muted=!enabled;if(enabled)void this.playAudio();this.changed();}
  async outputDevice(id){if(typeof this.audio.setSinkId!=="function")throw new Error("当前环境由系统选择扬声器");await this.audio.setSinkId(id);this.outputId=id;}
  async applyQuality(){
    const sender=this.video?.sender;if(!sender||this.closed||!this.session.local_host)return;
    try{const parameters=sender.getParameters();if(!parameters.encodings?.length)return;
      Object.assign(parameters.encodings[0],remoteQuality(this.session.quality));await sender.setParameters(parameters);
      const track=this.localScreen?.getVideoTracks()[0];if(track&&!this.native)await track.applyConstraints({frameRate:{max:remoteQuality(this.session.quality).maxFramerate}});
    }catch{/* Browser congestion control remains active when a codec cannot apply a hint. */}
  }
  bindChannel(channel){
    if(this.channel&&this.channel!==channel){channel.close();return;}this.channel=channel;
    channel.onmessage=event=>{
      if(this.closed||!this.native||!this.session.local_host||this.session.paused||typeof event.data!=="string"||event.data.length>2048)return;
      try{const packet=JSON.parse(event.data);if(packet.grant!==this.session.grant||!packet.grant)return;
        if(this.inputQueue.length>=64)throw new Error("远程输入积压，已收回控制");
        this.inputQueue.push(packet);void this.drainInput();
      }catch(error){void this.revokeInput(error);}
    };
    channel.onclose=()=>{if(!this.closed)this.fail(new Error("远程操作通道已断开"));};
  }
  async revokeInput(error){this.inputQueue=[];this.changed({inputError:error.message});await this.signal({type:"control",allow:false}).catch(()=>{});}
  async drainInput(){
    if(this.inputWorking)return;this.inputWorking=true;
    try{while(!this.closed&&this.inputQueue.length){const packet=this.inputQueue.shift();if(packet.grant!==this.session.grant)continue;try{await this.input(packet);}catch(error){if(!this.closed&&packet.grant===this.session.grant)await this.revokeInput(error);}}}
    finally{this.inputWorking=false;}
  }
  sendInput(event){
    if(!remoteInputAllowed(this.session)||this.channel?.readyState!=="open")return false;
    if(this.channel.bufferedAmount>65536)return false;
    if(event.type==="release")this.heldInputs.clear();
    else if(event.type==="key"||event.type==="button"){const key=event.type+(event.code??event.button);if(event.down)this.heldInputs.add(key);else this.heldInputs.delete(key);}
    this.holdingInput=this.heldInputs.size>0;
    this.channel.send(JSON.stringify({grant:this.session.grant,sequence:++this.inputSequence,event}));return true;
  }
  async stats(){
    if(this.closed)return;
    try{const reports=await this.pc.getStats();let info={};
      reports.forEach(report=>{
        if(report.type==="candidate-pair"&&report.state==="succeeded"&&report.nominated)info.rtt=report.currentRoundTripTime!=null?Math.round(report.currentRoundTripTime*1000):null;
        if(report.type===(this.session.local_host?"outbound-rtp":"inbound-rtp")&&report.kind==="video")Object.assign(info,{fps:report.framesPerSecond??null,width:report.frameWidth,height:report.frameHeight,bytes:report.bytesSent??report.bytesReceived});
      });
      if(this.previousStats&&info.bytes!=null)info.kbps=Math.max(0,Math.round((info.bytes-this.previousStats.bytes)*8/(performance.now()-this.previousStats.time)));
      if(info.bytes!=null)this.previousStats={bytes:info.bytes,time:performance.now()};this.metrics=info;this.changed();
    }catch{}
  }
  fail(error){if(this.closed)return;this.failed(error);this.close();}
  close(){
    if(this.closed)return;this.closed=true;this.audioEpoch++;clearInterval(this.statsTimer);clearInterval(this.inputKeepAlive);
    this.pc.onconnectionstatechange=null;if(this.channel)this.channel.onclose=null;
    this.channel?.close();this.pc.close();stop(this.localScreen);stop(this.microphone);stop(this.remoteScreen);
    this.audio.pause();this.audio.srcObject=null;this.inputQueue=[];this.changed();
  }
}
