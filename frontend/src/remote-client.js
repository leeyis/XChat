import {RemoteMedia} from "./remote-media.js";
import {remoteAccepted,remoteLive} from "./remote-model.js";
import {microphoneCapabilityError} from "./remote-voice.js";

const stop=stream=>stream?.getTracks().forEach(track=>track.stop());
const terminalPhases=new Set(['ended','cancelled','rejected','disconnected','expired','locked']);
export class RemoteClient {
  constructor({viewer=null}={}){
    this.state={available:false,ready:false,session:null,owned:false,error:"",voiceError:"",inputError:"",nativeHost:false};
    this.listeners=new Set();this.after=0;this.epoch=0;this.closedIds=new Set();this.dismissed=new Set();this.command=Promise.resolve();
    this.native=Boolean(globalThis.window?.__TAURI__);
    this.viewer=viewer;this.delegated=false;this.viewerState=null;
  }
  snapshot=()=>this.state;
  publish(extra={}){
    this.state={...this.state,localScreen:this.media?.localScreen||null,remoteScreen:this.media?.remoteScreen||null,
      microphone:!!this.media?.microphone,speaker:this.media?.speaker??true,audioBlocked:this.media?.audioBlocked||false,
      metrics:this.media?.metrics||{},connectionState:this.media?.pc?.connectionState||(terminalPhases.has(this.state.session?.phase)?"closed":"new"),
      ...(this.delegated?this.viewerState||{}:{}),...extra,delegated:this.delegated};
    this.listeners.forEach(listener=>listener());
    if(this.viewer){
      const state={};for(const key of ['microphone','speaker','audioBlocked','metrics','connectionState','error','voiceError','inputError'])state[key]=this.state[key];
      state.phase=this.state.session?.phase;
      this.postWindowMessage('state',{state});
    }
  }
  subscribe=listener=>{this.listeners.add(listener);void this.boot();return()=>this.listeners.delete(listener);};
  async rpc(method,request={}){
    if(this.native)return window.__TAURI__.core.invoke(`remote_${method}`,method==="bootstrap"?{}:{request});
    const response=await fetch(`/api/remote/ui/${method}`,{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify(request),signal:AbortSignal.timeout(method==="start"?25000:8000)});
    const result=await response.json();if(!response.ok)throw new Error(result.error||"远程协助请求失败");return result;
  }
  async boot(){
    if(this.booting)return this.booting;
    if(this.actor)return;
    this.booting=(async()=>{
      try {
        if(typeof RTCPeerConnection!=="function")throw new Error("当前环境不支持远程音视频连接");
        await this.listenWindowMessages();
        const result=this.viewer?{actor:this.viewer.actor,native_host:false}:await this.rpc("bootstrap");this.actor=result.actor;this.selfId=result.self_id;this.localInfo=result.local_info||null;
        if(this.viewer)this.configureBrowserChannel(this.viewer.id);
        this.publish({ready:true,available:true,nativeHost:result.native_host,error:""});
        this.timer=setInterval(()=>void this.poll(),700);void this.poll();
        if(!this.unload){this.unload=()=>this.leave();window.addEventListener("pagehide",this.unload);}
      }catch(error){this.publish({ready:true,available:false,error:String(error.message||error)});}
      finally{this.booting=null;}
    })();return this.booting;
  }
  async poll(){
    if(!this.actor||this.polling)return;this.polling=true;const epoch=this.epoch;
    try{
      const result=await this.rpc("poll",{actor:this.actor,id:this.viewer?.id||(this.delegated?null:this.state.session?.id)||null,after:this.delegated?0:this.after});
      if(epoch!==this.epoch)return;
      this.failures=0;await this.apply(result.session,result.owned);
      for(const event of result.signals||[]){
        if(this.media&&event.sequence>this.after){try{await this.media.handle(event.body);this.after=event.sequence;}catch(error){this.media.fail(error);throw error;}}
      }
    }catch(error){
      this.failures=(this.failures||0)+1;
      if(this.failures>=2&&remoteAccepted(this.state.session)&&!this.closedIds.has(this.state.session.id)){this.publish({error:"本机远程服务失联，共享已停止"});void this.act({type:"stop",reason:"disconnected"}).catch(()=>{});}
    }finally{this.polling=false;}
  }
  async apply(session,owned){
    if(this.viewer&&session?.id!==this.viewer.id){session=null;owned=false;}
    if(session&&!remoteLive(session)&&this.dismissed.has(session.id)){session=null;owned=false;}
    if(session&&this.state.session&&session.id===this.state.session.id&&session.version<this.state.session.version)return;
    if(session?.id!==this.state.session?.id){this.mediaEpoch=(this.mediaEpoch||0)+1;const previous=this.media;this.media=null;previous?.close();this.after=0;this.viewerState=null;this.openedViewerId=null;this.publish({error:"",voiceError:"",inputError:""});}
    if(session&&this.closedIds.has(session.id)&&remoteLive(session))session={...session,phase:"ended",grant:null,paused:true};
    if(session&&!remoteLive(session))this.closeReservedViewer();
    this.delegated=Boolean(!this.viewer&&owned&&remoteAccepted(session)&&!session.local_host);
    if(session)this.configureBrowserChannel(session.id);
    this.publish({session,owned:!!owned});
    if(this.delegated){
      if(this.native&&this.openedViewerId!==session.id){
        try{await this.openViewer();}catch(error){this.publish({error:String(error.message||error)});}
      }else if(!this.native&&this.remoteWindow&&!this.remoteWindow.closed&&this.openedViewerId!==session.id){
        await this.openViewer();
      }
      return;
    }
    if(owned&&remoteAccepted(session)){
      if(!this.media){
        const id=session.id,mediaEpoch=this.mediaEpoch=(this.mediaEpoch||0)+1;
        const current=()=>mediaEpoch===this.mediaEpoch&&this.state.session?.id===id;
        this.media=new RemoteMedia({session,native:this.state.nativeHost&&session.local_host,browserStream:this.source,localInfo:this.localInfo,
          signal:action=>this.act(action,id),
          frame:(revision,format,requestId,requestKeyframe)=>window.__TAURI__.core.invoke("remote_frame",{actor:this.actor,id,revision,format,requestId,requestKeyframe}),
          openCaptureStream:revision=>window.__TAURI__.core.invoke("remote_capture_stream",{actor:this.actor,id,revision}),
          input:packet=>window.__TAURI__.core.invoke("remote_input",{actor:this.actor,id,packet}),
          changed:extra=>{if(current())this.publish(extra);},
          failed:error=>{if(!current())return;this.publish({error:String(error.message||error)});void this.act({type:"stop",reason:"disconnected"},id).catch(()=>{});},
          voiceFailed:async (error,callId)=>{if(!current()||this.state.session.voice.id!==callId)return;this.publish({voiceError:String(error.message||error)});await this.act({type:"voice_end"},id).catch(()=>{});},
        });this.source=null;this.publish();
      }else this.media.update(session);
    }else if(!remoteLive(session)){this.mediaEpoch=(this.mediaEpoch||0)+1;const previous=this.media;this.media=null;previous?.close();stop(this.source);this.source=null;this.publish();}
  }
  async screens(){
    await this.boot();if(!this.actor)throw new Error(this.state.error);
    if(this.state.nativeHost)return window.__TAURI__.core.invoke("remote_screens",{actor:this.actor});
    return [];
  }
  async browserScreen(){
    if(!navigator.mediaDevices?.getDisplayMedia)throw new Error("当前环境无法共享屏幕，请使用 Windows 桌面客户端或支持屏幕共享的本机浏览器");
    const stream=await navigator.mediaDevices.getDisplayMedia({video:{frameRate:{ideal:30,max:30}},audio:false});
    const track=stream.getVideoTracks()[0],settings=track?.getSettings()||{};
    if(!track||!settings.width||!settings.height){stop(stream);throw new Error("系统没有提供可用的共享屏幕");}
    return {stream,screen:{id:track.id,name:track.label||"所选屏幕",width:settings.width,height:settings.height}};
  }
  setSource(stream){if(this.source!==stream)stop(this.source);this.source=stream;}
  discardSource(){stop(this.source);this.source=null;}
  dismiss(){if(this.state.session&&!remoteLive(this.state.session))this.dismissed.add(this.state.session.id);this.closeReservedViewer();this.publish({session:null,owned:false,error:"",voiceError:"",inputError:""});}
  async start(peerId,invitation){
    if(invitation.voice){const error=microphoneCapabilityError();if(error)throw error;}
    if(!this.native&&!this.viewer&&invitation.mode==='control')this.reserveViewer();
    try{
      await this.boot();if(!this.actor)throw new Error(this.state.error);this.epoch++;
      const session=await this.rpc("start",{actor:this.actor,peer_id:peerId,invitation:{...invitation,native_host:this.state.nativeHost}});
      await this.apply(session,true);this.publish({error:"",voiceError:"",inputError:""});return session;
    }catch(error){this.closeReservedViewer();throw error;}
  }
  act(action,id=this.state.session?.id){
    if(!id)return Promise.reject(new Error("远程会话已结束"));
    if(action.type==="voice_invite"||(action.type==="voice_answer"&&action.accepted)||(action.type==="accept"&&action.voice)){
      const error=microphoneCapabilityError();if(error)return Promise.reject(error);
    }
    const endingVoice=action.type==="voice_end"?this.state.session?.voice?.id:undefined;
    if(!this.native&&!this.viewer&&action.type==='accept'&&!this.state.session?.local_host)this.reserveViewer();
    const urgent=action.type==="stop"||action.type==="pause"||(action.type==="control"&&!action.allow);
    if(action.type==="stop"){
      this.closedIds.add(id);this.media?.close();this.discardSource();this.closeReservedViewer();
      if(this.state.session?.id===id)this.publish({session:{...this.state.session,phase:action.reason,grant:null,paused:true}});
    }
    const run=async()=>{
      if(action.type==="voice_end"&&(this.closedIds.has(id)||this.state.session?.id!==id||this.state.session?.voice?.id!==endingVoice))return this.state.session;
      this.epoch++;
      let session;
      try{session=await this.rpc("action",{actor:this.actor,id,action});}
      catch(error){if(action.type==='accept')this.closeReservedViewer();throw error;}
      await this.apply(session,true);return session;
    };
    if(urgent)return run();
    const operation=this.command.then(run);this.command=operation.catch(()=>{});return operation;
  }
  async switchBrowserScreen(){
    await this.act({type:"pause",paused:true});
    const {stream,screen}=await this.browserScreen();
    try{await this.act({type:"screen",screen});await this.media.changeBrowserScreen(stream);await this.act({type:"pause",paused:false});}
    catch(error){stop(stream);throw error;}
  }
  async listenWindowMessages(){
    if(!this.native||this.unlistenWindow)return;
    const event=this.viewer?'xchat:remote-media-action':'xchat:remote-viewer-state';
    this.unlistenWindow=await window.__TAURI__.event.listen(event,event=>this.receiveWindowMessage(event.payload));
  }
  configureBrowserChannel(id){
    if(this.native||this.channelId===id||typeof BroadcastChannel!=='function')return;
    this.windowChannel?.close();this.channelId=id;
    this.windowChannel=new BroadcastChannel(`xchat-remote-${id}`);
    this.windowChannel.onmessage=event=>this.receiveWindowMessage(event.data);
  }
  receiveWindowMessage(packet){
    const id=this.viewer?.id||this.state.session?.id;if(!packet||packet.id!==id)return;
    if(this.viewer&&packet.type==='action'){
      void this.mediaAction(packet.action).catch(error=>this.publish({voiceError:String(error.message||error)}));
    }else if(!this.viewer&&packet.type==='state'&&this.delegated){
      const state={};for(const key of ['microphone','speaker','audioBlocked','metrics','connectionState','error','voiceError','inputError'])if(key in (packet.state||{}))state[key]=packet.state[key];
      this.viewerState=state;this.publish();
      // A dying viewer can lose its stop invoke. Keep the main window's lease
      // from outliving that media owner, without importing its session or grant.
      const phase=packet.state?.phase;
      if(this.state.owned&&remoteAccepted(this.state.session)&&!this.closedIds.has(id)&&terminalPhases.has(phase)&&packet.state.connectionState==='closed'){
        void this.act({type:'stop',reason:phase},id).catch(()=>{});
      }
    }else if(!this.viewer&&packet.type==='chat'){
      window.focus();window.dispatchEvent(new CustomEvent('xchat:remote-chat',{detail:{peerId:this.state.session?.peer_id}}));
    }
  }
  postWindowMessage(type,body={}){
    const id=this.viewer?.id||this.state.session?.id;if(!id)return;
    const packet={id,type,...body};
    if(this.native){
      const target=this.viewer?'main':`remote-viewer-${id}`;
      const event=this.viewer?'xchat:remote-viewer-state':'xchat:remote-media-action';
      void window.__TAURI__.event.emitTo(target,event,packet).catch(()=>{});
    }else this.windowChannel?.postMessage(packet);
  }
  reserveViewer(){
    if(this.native||this.viewer||this.remoteWindow&&!this.remoteWindow.closed)return;
    this.remoteWindow=window.open('about:blank','xchat-remote-viewer','popup,width=1280,height=800');
    if(this.remoteWindow){this.openedViewerId=null;this.viewerReserved=true;this.remoteWindow.document.title='XChat 远程桌面';this.remoteWindow.document.body.textContent='正在等待对方同意远程协助…';}
  }
  closeReservedViewer(){
    if(!this.viewerReserved)return;
    this.remoteWindow?.close();this.remoteWindow=null;this.viewerReserved=false;this.openedViewerId=null;
  }
  async openViewer(){
    const session=this.state.session;if(!session||session.local_host||!remoteLive(session))return;
    if(this.viewer)return;
    if(this.openingViewer)return this.openingViewer;
    if(!this.native)this.reserveViewer();
    const operation=(async()=>{
      if(this.native)await window.__TAURI__.core.invoke('remote_open_viewer',{actor:this.actor,id:session.id});
      else {
        if(!this.remoteWindow||this.remoteWindow.closed)throw new Error('请允许此页面打开远程窗口，然后点击“打开远程窗口”');
        if(this.openedViewerId!==session.id){
          const url=new URL(window.location.href);url.search='';url.hash='';
          url.searchParams.set('view','remote-viewer');url.searchParams.set('actor',this.actor);url.searchParams.set('session',session.id);
          this.remoteWindow.location.href=url.href;
          this.viewerReserved=false;
        }
        this.remoteWindow.focus();
      }
      this.openedViewerId=session.id;
    })();
    this.openingViewer=operation;
    try{return await operation;}finally{if(this.openingViewer===operation)this.openingViewer=null;}
  }
  async focusChat(){
    const id=this.viewer?.id||this.state.session?.id;
    this.postWindowMessage('chat');
    if(this.native&&this.viewer)await window.__TAURI__.core.invoke('remote_focus_main',{id});
    else if(this.viewer)window.opener?.focus();
    else {window.focus();window.dispatchEvent(new CustomEvent('xchat:remote-chat',{detail:{peerId:this.state.session?.peer_id}}));}
  }
  async mediaAction(action){
    if(!action||!['speaker','play_audio','microphone','output'].includes(action.type))throw new Error('无效的音频操作');
    if(this.delegated){this.postWindowMessage('action',{action});return;}
    if(!this.media)throw new Error('远程音频通道尚未就绪');
    if(action.type==='speaker')this.media.setSpeaker(Boolean(action.enabled));
    if(action.type==='play_audio')await this.media.playAudio();
    if(action.type==='microphone')await this.media.syncVoice(action.deviceId||'');
    if(action.type==='output')await this.media.outputDevice(action.deviceId||'');
    this.publish();
  }
  leave(){
    const id=this.state.session?.id,shouldStop=id&&this.state.owned&&remoteLive(this.state.session);
    this.media?.close();this.discardSource();this.closeReservedViewer();clearInterval(this.timer);
    if(this.viewer&&shouldStop)this.publish({session:{...this.state.session,phase:'ended',grant:null,paused:true},connectionState:'closed'});
    this.unlistenWindow?.();this.unlistenWindow=null;this.windowChannel?.close();this.windowChannel=null;
    if(shouldStop){
      const request={actor:this.actor,id,action:{type:"stop",reason:"ended"}};
      if(this.native)void this.rpc("action",request).catch(()=>{});
      else void fetch("/api/remote/ui/action",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify(request),keepalive:true}).catch(()=>{});
    }
  }
}
export const remoteClient=new RemoteClient();
