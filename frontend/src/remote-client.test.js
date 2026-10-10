import test from 'node:test';
import assert from 'node:assert/strict';
import {RemoteClient} from './remote-client.js';

test('a queued failure from the previous voice call cannot hang up a redialled call',async()=>{
  const client=new RemoteClient(),requests=[];let release;
  client.state.session={id:'session',phase:'active',voice:{id:'old-call',stage:'active'}};
  client.command=new Promise(resolve=>{release=resolve;});
  client.rpc=async(method,request)=>{requests.push(request);return client.state.session;};
  client.apply=async()=>{};
  const ending=client.act({type:'voice_end'});
  client.state.session={...client.state.session,voice:{id:'redialled-call',stage:'active'}};
  release();await ending;
  assert.equal(requests.length,0);assert.equal(client.state.session.voice.id,'redialled-call');
});

test('voice end for the current call is delivered once with its original session',async()=>{
  const client=new RemoteClient(),requests=[];
  client.actor='owner';client.state.session={id:'session',phase:'active',voice:{id:'current',stage:'active'}};
  client.rpc=async(method,request)=>{requests.push({method,...request});return client.state.session;};
  client.apply=async()=>{};
  await client.act({type:'voice_end'});
  assert.deepEqual(requests,[{method:'action',actor:'owner',id:'session',action:{type:'voice_end'}}]);
});

test('a stopped session discards its queued voice hangup',async()=>{
  const client=new RemoteClient();let release,requested=false;
  client.state.session={id:'session',phase:'active',voice:{id:'call',stage:'active'}};
  client.command=new Promise(resolve=>{release=resolve;});
  client.rpc=async()=>{requested=true;};
  const ending=client.act({type:'voice_end'});client.closedIds.add('session');
  release();await ending;assert.equal(requested,false);
});

test('main controller delegates one media owner to the native viewer',async()=>{
  const client=new RemoteClient();client.native=true;let opened=0;
  client.openViewer=async()=>{opened++;client.openedViewerId='session';};
  const session={id:'session',version:1,phase:'connecting',local_host:false,voice:{id:null,stage:'idle'}};
  await client.apply(session,true);await client.apply({...session,version:2,phase:'active'},true);
  assert.equal(opened,1);assert.equal(client.delegated,true);assert.equal(client.media,null);
  let request;client.actor='owner';client.rpc=async(method,value)=>{request=value;return{session:{...session,version:3},owned:true,signals:[]};};
  await client.poll();assert.equal(request.id,null);assert.equal(request.after,0);
});

test('viewer is bound to its original session and never consumes a replacement session',async()=>{
  const client=new RemoteClient({viewer:{actor:'owner',id:'old-session'}});
  client.native=true;client.postWindowMessage=()=>{};
  await client.apply({id:'new-session',version:1,phase:'active',local_host:false},true);
  assert.equal(client.state.session,null);assert.equal(client.state.owned,false);assert.equal(client.media,null);
});

test('main accepts only current viewer diagnostics and never imports stream or grant data',()=>{
  const client=new RemoteClient();client.state.session={id:'session',grant:null};client.delegated=true;
  client.receiveWindowMessage({id:'old',type:'state',state:{microphone:true}});
  assert.equal(client.viewerState,null);
  client.receiveWindowMessage({id:'session',type:'state',state:{microphone:true,metrics:{fps:30},session:{grant:'foreign'},localScreen:'foreign'}});
  assert.equal(client.state.microphone,true);assert.equal(client.state.metrics.fps,30);
  assert.equal(client.state.session.grant,null);assert.equal(client.state.localScreen,null);
});

test('reopening a closed browser viewer navigates a fresh popup for the same session',async()=>{
  const previous=globalThis.window;
  const popup={closed:false,document:{body:{}},location:{href:'about:blank'},focus(){},close(){this.closed=true;}};
  globalThis.window={open:()=>popup,location:{href:'http://localhost:8888/'}};
  try{
    const client=new RemoteClient();client.actor='owner';client.openedViewerId='session';client.remoteWindow={closed:true};
    client.state.session={id:'session',phase:'active',local_host:false};
    await client.openViewer();
    assert.equal(new URL(popup.location.href).searchParams.get('session'),'session');
    assert.equal(client.viewerReserved,false);
    client.closeReservedViewer();assert.equal(popup.closed,false);
  }finally{globalThis.window=previous;}
});

test('a failed or rejected invitation closes only its unused browser placeholder',async()=>{
  const previous=globalThis.window;const popups=[];
  globalThis.window={open:()=>{const popup={closed:false,document:{body:{}},close(){this.closed=true;}};popups.push(popup);return popup;}};
  const client=new RemoteClient();
  try{
    client.boot=async()=>{};client.actor='owner';client.rpc=async()=>{throw new Error('peer unavailable');};
    await assert.rejects(client.start('peer',{mode:'control',voice:false}),/peer unavailable/);
    assert.equal(popups[0].closed,true);assert.equal(client.remoteWindow,null);
    client.reserveViewer();await client.apply({id:'session',version:1,phase:'rejected',local_host:false},true);
    assert.equal(popups[1].closed,true);assert.equal(client.remoteWindow,null);
  }finally{client.windowChannel?.close();globalThis.window=previous;}
});

test('viewer terminal state remains closed after its media has been released',()=>{
  const client=new RemoteClient({viewer:{actor:'owner',id:'session'}}),packets=[];
  client.state.session={id:'session',phase:'disconnected'};
  client.postWindowMessage=(type,body)=>packets.push({type,...body});
  client.publish({error:'codec negotiation failed'});
  assert.equal(packets[0].state.phase,'disconnected');
  assert.equal(packets[0].state.connectionState,'closed');
  assert.equal(packets[0].state.error,'codec negotiation failed');
  assert.equal('session' in packets[0].state,false);
});

test('main converges a failed viewer to one stop without importing viewer session data',async()=>{
  const client=new RemoteClient(),requests=[];
  client.actor='owner';client.delegated=true;
  client.state={...client.state,owned:true,session:{id:'session',version:1,phase:'connecting',local_host:false,grant:null}};
  client.rpc=async(method,request)=>{requests.push({method,...request});return{...client.state.session,version:2,phase:request.action.reason};};
  const packet={id:'session',type:'state',state:{phase:'disconnected',connectionState:'closed',error:'codec negotiation failed',session:{grant:'foreign'}}};
  client.receiveWindowMessage(packet);client.receiveWindowMessage(packet);
  await new Promise(resolve=>setImmediate(resolve));
  assert.deepEqual(requests,[{method:'action',actor:'owner',id:'session',action:{type:'stop',reason:'disconnected'}}]);
  assert.equal(client.state.session.phase,'disconnected');
  assert.equal(client.state.session.grant,null);
  assert.equal(client.state.error,'codec negotiation failed');
  assert.equal(client.delegated,false);assert.equal(client.media,null);
  client.windowChannel?.close();
});

test('old viewer termination and incomplete terminal diagnostics cannot stop the current session',()=>{
  const client=new RemoteClient();let stops=0;
  client.delegated=true;client.state={...client.state,owned:true,session:{id:'current',phase:'active',local_host:false}};
  client.act=async()=>{stops++;};
  for(const packet of [
    {id:'old',state:{phase:'ended',connectionState:'closed'}},
    {id:'current',state:{phase:'active',connectionState:'closed'}},
    {id:'current',state:{phase:'disconnected',connectionState:'connected'}},
    {id:'current',state:{phase:'invalid',connectionState:'closed'}},
  ])client.receiveWindowMessage({type:'state',...packet});
  assert.equal(stops,0);assert.equal(client.state.session.phase,'active');
});

test('viewer leave relays ended before closing its channel and keeps one unload stop fallback',async()=>{
  const priorFetch=globalThis.fetch,events=[],requests=[];
  globalThis.fetch=async(url,options)=>{requests.push(JSON.parse(options.body));return{};};
  try{
    const client=new RemoteClient({viewer:{actor:'owner',id:'session'}});client.actor='owner';
    client.state={...client.state,owned:true,session:{id:'session',phase:'active',grant:'local'}};
    client.postWindowMessage=(type,body)=>events.push({type,...body});
    client.windowChannel={close:()=>events.push({type:'channel-closed'})};
    client.leave();client.leave();
    assert.equal(events[0].type,'state');assert.equal(events[0].state.phase,'ended');assert.equal(events[0].state.connectionState,'closed');
    assert.equal(events[1].type,'channel-closed');
    assert.deepEqual(requests,[{actor:'owner',id:'session',action:{type:'stop',reason:'ended'}}]);
    assert.equal(client.state.session.grant,null);
  }finally{globalThis.fetch=priorFetch;}
});
