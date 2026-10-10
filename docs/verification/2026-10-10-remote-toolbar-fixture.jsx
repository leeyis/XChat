// Browser QA fixture only. It imports production components, never ships in the app entry.
import React,{useRef,useState} from 'react';
import {createRoot} from 'react-dom/client';
import '../../frontend/src/styles.css';
import '../../frontend/src/remote.css';
import RemoteSessionView from '../../frontend/src/RemoteSessionView.jsx';
import {newPanelMemory,clearPanelCollapsed} from '../../frontend/src/remote-panel.js';

function App() {
  const role=new URLSearchParams(location.search).get('role')||'viewer';
  const [session,setSession]=useState({id:'toolbar-qa-'+role,local_host:role==='host',phase:'active',peer_name:'张三',screen:{name:'屏幕 1'},grant:'qa-grant',paused:false,native_host:true,control_requested:false,quality:{preset:'auto',fps:30},voice:{stage:'active',started_at:Date.now()/1000,local_muted:false,peer_muted:false},started_at:Date.now()/1000});
  const [scale,setScale]=useState('fit'),[chat,setChat]=useState(false),[draft,setDraft]=useState(''),[ended,setEnded]=useState(false),[show,setShow]=useState(true);
  const memory=useRef(newPanelMemory());
  const action=a=>{
    if(['control','release_control'].includes(a.type))setSession(s=>({...s,grant:null}));
    if(a.type==='pause')setSession(s=>({...s,paused:a.paused,grant:null}));
    if(a.type==='muted')setSession(s=>({...s,voice:{...s.voice,local_muted:a.muted}}));
  };
  window.qa={session,action,setSession,setShow,setEnded,memory:memory.current,clear:()=>clearPanelCollapsed(session.id)};
  return <section className="remote-workspace remote-session" style={{inset:'40px 0 0 56px'}}>{!ended&&show&&<RemoteSessionView remote={{session,microphone:{},speaker:true,metrics:{rtt:12,fps:30,width:1920,height:1080},connectionState:'connected'}} name="张三" localName="Eason" busy={false} scale={scale} setScale={setScale} chat={chat} setChat={setChat} draft={draft} setDraft={setDraft} messages={[]} conversation={{id:'qa'}} onSendChat={()=>{}} onEnd={()=>setEnded(true)} onMinimize={()=>setShow(false)} onAction={action} onQuality={()=>{window.qaQualityOpened=true;}} onScreen={()=>{}} onControl={()=>{}} onAudio={()=>{}} onFull={()=>{}} fullscreen={false} memory={memory.current} releaseInput={()=>{window.qaReleases=(window.qaReleases||0)+1;}}><div className="remote-video-viewport" style={{background:'#242b30'}}/></RemoteSessionView>}</section>;
}
const params=new URLSearchParams(location.search);document.documentElement.dataset.theme=params.get('theme')||'light';
clearPanelCollapsed('toolbar-qa-'+(params.get('role')||'viewer'));
createRoot(document.getElementById('root')).render(<App/>);
