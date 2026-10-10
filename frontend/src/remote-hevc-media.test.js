import test from 'node:test';
import assert from 'node:assert/strict';
import {RemoteHevcMedia,createHevcControlOutbox} from './remote-hevc-media.js';

class ControlChannel extends EventTarget {
  constructor(state='open'){super();this.label='xchat-control';this.readyState=state;this.bufferedAmount=0;this.sent=[];}
  send(message){this.sent.push(JSON.parse(message));}
}
const control=(action,fields={})=>({type:'xchat-hevc',version:1,sessionId:'session',action,...fields});

test('fallback restores the RTP track after an already pending HEVC detach',async()=>{
  let completeDetach;const changes=[],track={id:'fallback'};
  const adapter=Object.create(RemoteHevcMedia.prototype);
  Object.assign(adapter,{closed:false,trackSync:Promise.resolve(),sender:{ready:true,failed:false},
    media:{closed:false,changed(){},localScreen:{getVideoTracks:()=>[track]},
      video:{sender:{replaceTrack(value){changes.push(value);return value?Promise.resolve():new Promise(resolve=>{completeDetach=resolve;});}}}},
  });
  const detach=adapter.detachRtp();
  await new Promise(resolve=>setImmediate(resolve));
  assert.deepEqual(changes,[null]);
  adapter.sender.failed=true;const restore=adapter.restoreRtp();
  completeDetach();await detach;await restore;
  assert.deepEqual(changes,[null,track]);assert.equal(adapter.detached,false);
});

test('pause or screen revision invalidates HEVC on both sides before later output',()=>{
  const resets=[];const adapter=Object.create(RemoteHevcMedia.prototype);
  Object.assign(adapter,{closed:false,sender:{reset(){resets.push('sender');}},
    receiver:{reset(state){resets.push(state);}},
    media:{closed:false,session:{revision:3,paused:true}},
  });
  adapter.update(adapter.media.session,{revision:2,paused:false});
  assert.deepEqual(resets,['sender',{revision:3,paused:true}]);
  // These frames must be rejected before allocating or drawing a canvas.
  adapter.present({}, {revision:3});
  adapter.media.session.paused=false;adapter.present({}, {revision:2});
  assert.equal(adapter.canvas,undefined);
  adapter.update(adapter.media.session,{revision:3,paused:false});
  assert.equal(resets.length,2);
});

test('a Worker that relinquishes capture resumes RTP even without a permanent HEVC failure',async()=>{
  const changes=[],track={id:'rtp'};const adapter=Object.create(RemoteHevcMedia.prototype);
  Object.assign(adapter,{closed:false,detached:true,wantDetached:true,trackSync:Promise.resolve(),
    sender:{supported:true,failed:false,ownsCapture:true,capture:async()=>false},
    media:{closed:false,session:{revision:1,paused:false},changed(){},localScreen:{getVideoTracks:()=>[track]},
      video:{sender:{async replaceTrack(value){changes.push(value);}}}},
  });
  assert.equal(await adapter.capture(1),false);
  assert.deepEqual(changes,[track]);assert.equal(adapter.detached,false);assert.equal(adapter.wantDetached,false);
});

test('an old capture completion cannot resume RGBA during a newer revision or pause',async()=>{
  let release;const adapter=Object.create(RemoteHevcMedia.prototype);
  Object.assign(adapter,{closed:false,detached:true,wantDetached:true,
    sender:{supported:true,failed:false,ownsCapture:true,capture:()=>new Promise(resolve=>{release=resolve;})},
    media:{closed:false,session:{revision:1,paused:false}},
    restoreRtp(){assert.fail('old capture must not replace a current track');},
  });
  const capture=adapter.capture(1);adapter.media.session={revision:2,paused:true};release(false);
  assert.equal(await capture,true);
});

test('pending and failed HEVC upgrades expose diagnostics without claiming HEVC video output',()=>{
  const adapter=Object.create(RemoteHevcMedia.prototype),diagnostic={phase:'authorizing',lastProgressAgeMs:4000};
  Object.assign(adapter,{sender:{ready:false,stats:()=>({hevcWorker:diagnostic,framesSent:0,codecString:'hev1.1.6.H120.90'})},receiving:false});
  assert.deepEqual(adapter.stats(),{hevcWorker:diagnostic});
  adapter.fallbackReason='HEVC native capture authorization descriptor timed out';
  assert.equal(adapter.stats().hevcFallbackReason,adapter.fallbackReason);
  assert.equal(adapter.stats().codec,undefined);assert.equal(adapter.stats().fps,undefined);
});

test('control replacement flushes only the newest ACK and metrics without restoring input permission',()=>{
  const old=new ControlChannel('closed'),media={session:{id:'session'},channel:old,inputSuspended:true};
  const outbox=createHevcControlOutbox(media);outbox.bind(old);
  try {
    for(const sequence of ['1','2','10','3'])assert.equal(outbox.send(control('frame-ack',{streamId:'stream',sequence})),true);
    for(const count of [1,2,3])assert.equal(outbox.send(control('metrics',{streamId:'stream',metadata:{count}})),true);
    assert.equal(old.sent.length,0);
    const channel=new ControlChannel();outbox.bind(channel);media.channel=channel;outbox.flush();
    assert.deepEqual(channel.sent.map(message=>[message.action,message.sequence??message.metadata.count]),[['frame-ack','10'],['metrics',3]]);
    assert.equal(media.inputSuspended,true);assert.equal(channel.bufferedAmountLowThreshold,8192);
  }finally{outbox.close();}
});

test('bufferedamountlow drains bounded HEVC control messages after transient congestion',()=>{
  const channel=new ControlChannel(),media={session:{id:'session'},channel};channel.bufferedAmount=20000;
  const outbox=createHevcControlOutbox(media);outbox.bind(channel);
  try {
    assert.equal(outbox.send(control('config',{streamId:'stream'})),true);assert.equal(channel.sent.length,0);
    channel.bufferedAmount=0;channel.dispatchEvent(new Event('bufferedamountlow'));
    assert.equal(channel.sent.length,1);assert.equal(channel.sent[0].action,'config');
  }finally{outbox.close();}
});

test('control backlog is bounded by entries and bytes, and rejects other sessions',()=>{
  const channel=new ControlChannel('closed'),outbox=createHevcControlOutbox({session:{id:'session'},channel});
  try {
    for(let i=0;i<8;i++)assert.equal(outbox.send(control('config-ack',{streamId:String(i),supported:true})),true);
    assert.equal(outbox.send(control('config-ack',{streamId:'overflow',supported:true})),false);
    assert.equal(outbox.send({...control('frame-ack'),sessionId:'foreign'}),false);
    outbox.reset();let accepted=0;
    for(let i=0;i<8;i++)accepted+=Number(outbox.send(control('metrics',{streamId:String(i),metadata:{text:'a'.repeat(3000)}})));
    assert.equal(accepted,5);
    assert.equal(outbox.send(control('metrics',{streamId:'large',metadata:{text:'a'.repeat(4096)}})),false);
  }finally{outbox.close();}
});

test('revision reset and close discard old controls instead of replaying them on a replacement channel',()=>{
  const old=new ControlChannel('closed'),media={session:{id:'session'},channel:old},outbox=createHevcControlOutbox(media);
  const adapter=Object.assign(Object.create(RemoteHevcMedia.prototype),{closed:false,controls:outbox,keyframe:false,
    receiver:{reset(scope){outbox.send(control('reset',scope));},probe:async()=>true},media});
  outbox.send(control('frame-ack',{streamId:'old',sequence:'99'}));
  adapter.update({revision:2,paused:true},{revision:1,paused:false});
  const channel=new ControlChannel();adapter.bindChannel(channel);media.channel=channel;adapter.controlOpen();
  assert.deepEqual(channel.sent,[control('reset',{revision:2,paused:true})]);
  channel.readyState='closed';outbox.send(control('frame-ack',{streamId:'new',sequence:'1'}));
  outbox.close();channel.readyState='open';outbox.flush();channel.dispatchEvent(new Event('bufferedamountlow'));
  assert.equal(channel.sent.length,1);assert.equal(outbox.send(control('probe')),false);
});
