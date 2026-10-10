import {RemoteMedia} from "./remote-media.js";
import {remoteAccepted,remoteLive} from "./remote-model.js";

const stop=stream=>stream?.getTracks().forEach(track=>track.stop());
export class RemoteClient {
  constructor(){
    this.state={available:false,ready:false,session:null,owned:false,error:"",voiceError:"",inputError:"",nativeHost:false};
    this.listeners=new Set();this.after=0;this.epoch=0;this.closedIds=new Set();this.dismissed=new Set();this.command=Promise.resolve();
    this.native=Boolean(globalThis.window?.__TAURI__);
  }
  snapshot=()=>this.state;
  publish(extra={}){
    this.state={...this.state,...extra,localScreen:this.media?.localScreen||null,remoteScreen:this.media?.remoteScreen||null,
      microphone:!!this.media?.microphone,speaker:this.media?.speaker??true,audioBlocked:this.media?.audioBlocked||false,
      metrics:this.media?.metrics||{},connectionState:this.media?.pc?.connectionState||"new"};
    this.listeners.forEach(listener=>listener());
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
        const result=await this.rpc("bootstrap");this.actor=result.actor;this.selfId=result.self_id;
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
      const result=await this.rpc("poll",{actor:this.actor,id:this.state.session?.id||null,after:this.after});
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
    if(session&&!remoteLive(session)&&this.dismissed.has(session.id)){session=null;owned=false;}
    if(session?.id===this.state.session?.id&&session.version<this.state.session.version)return;
    if(session?.id!==this.state.session?.id){this.media?.close();this.media=null;this.after=0;this.publish({voiceError:"",inputError:""});}
    if(session&&this.closedIds.has(session.id)&&remoteLive(session))session={...session,phase:"ended",grant:null,paused:true};
    this.publish({session,owned:!!owned});
    if(owned&&remoteAccepted(session)){
      if(!this.media){
        const id=session.id;
        this.media=new RemoteMedia({session,native:this.state.nativeHost&&session.local_host,browserStream:this.source,
          signal:action=>this.act(action,id),
          frame:revision=>window.__TAURI__.core.invoke("remote_frame",{actor:this.actor,id,revision}),
          input:packet=>window.__TAURI__.core.invoke("remote_input",{actor:this.actor,id,packet}),
          changed:extra=>this.publish(extra),
          failed:error=>{this.publish({error:String(error.message||error)});void this.act({type:"stop",reason:"disconnected"},id).catch(()=>{});},
          voiceFailed:async error=>{this.publish({voiceError:String(error.message||error)});await this.act({type:"voice_end"},id).catch(()=>{});},
        });this.source=null;
      }else this.media.update(session);
    }else if(!remoteLive(session)){this.media?.close();this.media=null;stop(this.source);this.source=null;this.publish();}
  }
  async screens(){
    await this.boot();if(!this.actor)throw new Error(this.state.error);
    if(this.state.nativeHost)return window.__TAURI__.core.invoke("remote_screens",{actor:this.actor});
    return [];
  }
  async browserScreen(){
    if(!navigator.mediaDevices?.getDisplayMedia)throw new Error("当前环境无法共享屏幕，请使用 Windows 桌面客户端或支持屏幕共享的本机浏览器");
    const stream=await navigator.mediaDevices.getDisplayMedia({video:{frameRate:{ideal:20,max:30}},audio:false});
    const track=stream.getVideoTracks()[0],settings=track?.getSettings()||{};
    if(!track||!settings.width||!settings.height){stop(stream);throw new Error("系统没有提供可用的共享屏幕");}
    return {stream,screen:{id:track.id,name:track.label||"所选屏幕",width:settings.width,height:settings.height}};
  }
  setSource(stream){if(this.source!==stream)stop(this.source);this.source=stream;}
  discardSource(){stop(this.source);this.source=null;}
  dismiss(){if(this.state.session&&!remoteLive(this.state.session))this.dismissed.add(this.state.session.id);this.publish({session:null,owned:false,error:"",voiceError:"",inputError:""});}
  async start(peerId,invitation){
    await this.boot();if(!this.actor)throw new Error(this.state.error);this.epoch++;
    const session=await this.rpc("start",{actor:this.actor,peer_id:peerId,invitation:{...invitation,native_host:this.state.nativeHost}});
    await this.apply(session,true);this.publish({error:"",voiceError:"",inputError:""});return session;
  }
  act(action,id=this.state.session?.id){
    if(!id)return Promise.reject(new Error("远程会话已结束"));
    const urgent=action.type==="stop"||action.type==="pause"||(action.type==="control"&&!action.allow);
    if(action.type==="stop"){
      this.closedIds.add(id);this.media?.close();this.discardSource();
      if(this.state.session?.id===id)this.publish({session:{...this.state.session,phase:action.reason,grant:null,paused:true}});
    }
    const run=async()=>{
      this.epoch++;
      const session=await this.rpc("action",{actor:this.actor,id,action});
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
  leave(){
    const id=this.state.session?.id;this.media?.close();this.discardSource();clearInterval(this.timer);
    if(id&&this.state.owned&&remoteLive(this.state.session)){
      const request={actor:this.actor,id,action:{type:"stop",reason:"ended"}};
      if(this.native)void this.rpc("action",request).catch(()=>{});
      else void fetch("/api/remote/ui/action",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify(request),keepalive:true}).catch(()=>{});
    }
  }
}
export const remoteClient=new RemoteClient();
