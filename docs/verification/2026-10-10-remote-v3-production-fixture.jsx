// QA only: production React surfaces with an isolated in-memory session.
import React,{useRef,useSyncExternalStore} from 'react';
import {createRoot} from 'react-dom/client';
import App from '../../frontend/src/App.jsx';
import RemoteViewer from '../../frontend/src/RemoteViewer.jsx';
import {RemoteFloatingPanel,SharingControls} from '../../frontend/src/RemoteControls.jsx';
import {remoteClient} from '../../frontend/src/remote-client.js';
import {newPanelMemory,clearPanelCollapsed} from '../../frontend/src/remote-panel.js';
import {createXChatModule} from '../../frontend/src/xchat.js';
import '../../frontend/src/styles.css';

const role=new URLSearchParams(location.search).get('role')||'viewer',host=role==='host',peerId='qa-remote-peer';
let remote={ready:true,available:true,nativeHost:true,owned:true,delegated:!host,microphone:true,speaker:true,audioBlocked:false,metrics:{},connectionState:'connected',error:'',voiceError:'',inputError:'',
  session:{id:'remote-v3-qa-'+role,phase:'active',version:1,revision:1,initiator:true,local_host:host,native_host:true,peer_id:peerId,peer_name:host?'Eason-MBP(M5)':'Eason-Windows',mode:'control',screen:{id:'display1',name:'DISPLAY1',width:1280,height:720},quality:{preset:'auto',fps:30,reduced_color:false},grant:'qa-grant',paused:false,control_requested:false,started_at:Date.now()/1000-168,voice:{stage:'active',id:'qa-voice',started_at:Date.now()/1000-138,local_muted:false,peer_muted:false}}};
const listeners=new Set(),calls=[];
const publish=extra=>{remote={...remote,...extra};listeners.forEach(listener=>listener());};
const act=async action=>{calls.push(action);const session={...remote.session,version:remote.session.version+1};
  if(action.type==='control')session.grant=action.allow?'qa-grant':null;
  if(action.type==='release_control')session.grant=null;
  if(action.type==='offer_control')session.grant='qa-grant';
  if(action.type==='pause'){session.paused=action.paused;session.grant=null;}
  if(action.type==='muted')session.voice={...session.voice,local_muted:action.muted};
  if(action.type==='voice_end')session.voice={...session.voice,stage:'idle'};
  if(action.type==='voice_invite')session.voice={...session.voice,stage:'ringing',local_caller:true};
  if(action.type==='quality')session.quality=action.quality;
  if(action.type==='stop'){session.phase='ended';session.grant=null;}
  publish({session});return session;
};
const client={snapshot:()=>remote,subscribe:listener=>{listeners.add(listener);return()=>listeners.delete(listener);},act,publish,
  media:{sendInput:packet=>calls.push({type:'input',packet})},
  openViewer:async()=>calls.push({type:'open_viewer'}),focusChat:async()=>calls.push({type:'focus_chat'}),
  mediaAction:async action=>{calls.push(action);if(action.type==='speaker')publish({speaker:action.enabled});if(action.type==='play_audio')publish({audioBlocked:false});},
  screens:async()=>[{id:'display1',name:'DISPLAY1',width:1280,height:720}],dismiss:()=>publish({session:null}),discardSource:()=>{},boot:async()=>{},
};
Object.assign(remoteClient,client);
if(role==='viewer'){
  const canvas=document.createElement('canvas');canvas.width=1280;canvas.height=720;
  const context=canvas.getContext('2d');context.fillStyle='#ccdedc';context.fillRect(0,0,1280,720);
  context.fillStyle='#f7faf9';context.fillRect(190,80,900,540);context.fillStyle='#eaf0ed';context.fillRect(190,80,235,540);
  context.fillStyle='#4e685d';context.font='25px sans-serif';context.fillText('网络和 Internet',470,165);
  context.font='15px sans-serif';context.fillText('Eason-Windows',220,140);context.fillText('系统',218,220);context.fillText('网络和 Internet',218,269);
  for(let y=230;y<520;y+=95){context.strokeStyle='#dce5df';context.strokeRect(470,y,575,75);context.fillText(y===230?'以太网':y===325?'高级网络设置':'网络疑难解答',490,y+35);}
  context.font='13px sans-serif';context.fillText('生产组件验证 · 合成画面，不代表真实传输性能',850,690);
  remote.remoteScreen=canvas.captureStream(1);
}
const baseline=createXChatModule().getSnapshot();
let chatState={...baseline,phase:'ready',activeSection:'chat',activeConversationId:'qa-chat',self:{...baseline.self,id:'qa-self',name:'我'},settings:{...baseline.settings,theme:'light'},
  devices:[{id:peerId,name:remote.session.peer_name,addr:'192.168.1.102:8888',is_offline:false,connection_state:'ready',last_seen:Date.now()/1000,discovery_source:'lan'}],
  conversations:[{id:'qa-chat',kind:'direct',peer_id:peerId,name:remote.session.peer_name,last_message:'我这边已经准备好了',unread_count:0}],
  messagesByConversation:{'qa-chat':[{id:1,client_message_id:'qa1',conversation_id:'qa-chat',sender_id:peerId,sender_name:remote.session.peer_name,content:'可以帮我看看网络设置吗？',msg_type:'text',timestamp:Date.now()/1000-180,status:'sent',own:false},{id:2,client_message_id:'qa2',conversation_id:'qa-chat',sender_id:'qa-self',sender_name:'我',content:'可以，我打开远程桌面看一下。',msg_type:'text',timestamp:Date.now()/1000-150,status:'sent',own:true}]} };
const workspaceListeners=new Set();
const patchChat=extra=>{chatState={...chatState,...extra};workspaceListeners.forEach(listener=>listener());};
const workspace={getSnapshot:()=>chatState,subscribe:listener=>{workspaceListeners.add(listener);return()=>workspaceListeners.delete(listener);},dispatch:async action=>{calls.push(action);if(action.type==='navigation.open')patchChat({activeSection:action.section});if(action.type==='conversation.open')patchChat({activeConversationId:action.id});return{ok:true,value:chatState};}};
function HostPill(){const current=useSyncExternalStore(client.subscribe,client.snapshot),memory=useRef(newPanelMemory());return <div className="remote-native-shell" style={{position:'fixed',top:9,left:'50%',transform:'translateX(-50%)',zIndex:100}}><RemoteFloatingPanel session={current.session} memory={memory.current} onRevoke={()=>act({type:'control',allow:false})} onEnd={()=>act({type:'stop'})}>{props=><SharingControls {...props} session={current.session} onRevoke={()=>act({type:'control',allow:false})} onPause={()=>act({type:'pause',paused:!current.session.paused})} onEnd={()=>act({type:'stop'})}/>}</RemoteFloatingPanel></div>;}
window.qa={calls,publish,act,patchChat,get chat(){return chatState;},get remote(){return remote;}};
clearPanelCollapsed(remote.session.id);
document.documentElement.dataset.theme='light';
createRoot(document.getElementById('root')).render(role==='viewer'?<RemoteViewer client={client}/>:<><App workspace={workspace}/>{host&&<HostPill/>}</>);
