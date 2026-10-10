import test from 'node:test';
import assert from 'node:assert/strict';
import {watchRemoteConnection} from './remote-connection.js';

function setup(localHost=true) {
  const tasks=new Map(),signals=[],errors=[],changes=[];
  const media={closed:false,pc:{connectionState:'connected'},session:{local_host:localHost,grant:'approved'},
    inputQueue:[{}],holdingInput:true,heldInputs:new Set(['keyA']),
    async signal(action){signals.push(action);},fail(error){errors.push(error);},changed(change){changes.push(change);}};
  let next=0;
  const watcher=watchRemoteConnection(media,{setTimer:fn=>{tasks.set(++next,fn);return next;},clearTimer:id=>tasks.delete(id)});
  const state=value=>{media.pc.connectionState=value;media.pc.onconnectionstatechange?.();};
  return {media,watcher,tasks,signals,errors,changes,state};
}

test('temporary ICE disconnect revokes held control without ending screen or voice',()=>{
  const x=setup();x.state('disconnected');
  assert.equal(x.errors.length,0);assert.equal(x.media.heldInputs.size,0);
  assert.deepEqual(x.signals,[{type:'control',allow:false}]);assert.equal(x.tasks.size,1);
  x.state('connected');assert.equal(x.tasks.size,0);assert.equal(x.errors.length,0);
  assert.deepEqual(x.signals.at(-1),{type:'ready'});
});

test('persistent ICE disconnect is bounded and terminal failures end immediately',()=>{
  const x=setup();x.state('disconnected');x.state('disconnected');assert.equal(x.tasks.size,1);
  [...x.tasks.values()][0]();assert.equal(x.errors.length,1);
  const y=setup();y.state('failed');assert.equal(y.errors.length,1);assert.equal(y.tasks.size,0);
});

test('input closure only revokes control and stale channels cannot affect a replacement',()=>{
  const x=setup(false),old={},next={};x.media.channel=old;x.watcher.bind(old);
  old.onclose();assert.equal(x.errors.length,0);assert.deepEqual(x.signals,[{type:'release_control'}]);
  assert.match(x.changes.at(-1).inputError,/操作通道/);
  x.media.channel=next;x.watcher.bind(next);const count=x.changes.length;
  old.onclose();assert.equal(x.changes.length,count);
  next.onopen();assert.deepEqual(x.changes.at(-1),{inputError:''});
  assert.equal(x.media.inputSuspended,true);
});

test('teardown cancels timers and ignores delayed channel closure',()=>{
  const x=setup(),channel={};x.media.channel=channel;x.watcher.bind(channel);x.state('disconnected');
  x.watcher.dispose();const count=x.changes.length;channel.onclose();
  assert.equal(x.tasks.size,0);assert.equal(x.media.pc.onconnectionstatechange,null);assert.equal(x.changes.length,count);
});

test('initiator replaces a closed input channel without restarting the media session',()=>{
  const x=setup(),old={readyState:'closed'},next={readyState:'connecting'};
  x.media.session.initiator=true;x.media.channel=old;x.watcher.bind(old);
  x.media.pc.createDataChannel=(label,options)=>{assert.equal(label,'xchat-control');assert.equal(options.ordered,true);return next;};
  x.media.bindChannel=channel=>{x.media.channel=channel;x.watcher.bind(channel);};
  old.onclose();assert.equal(x.tasks.size,1);[...x.tasks.values()][0]();
  assert.equal(x.media.channel,next);next.readyState='open';next.onopen();
  assert.deepEqual(x.changes.at(-1),{inputError:''});assert.equal(x.errors.length,0);
  // The already revoked grant is never reissued by reopening SCTP.
  assert.deepEqual(x.signals,[{type:'control',allow:false}]);
  assert.equal(x.media.inputSuspended,true);
});
