import test from "node:test";
import assert from "node:assert/strict";
import {RemoteMedia} from "./remote-media.js";

const stream=()=>{const track={ended:false,stop(){this.ended=true;},addEventListener(){}};return{getTracks:()=>[track],getAudioTracks:()=>[track]};};
const media=()=>({audioEpoch:0,session:{voice:{stage:"active",id:"call",local_muted:false}},changed(){},voiceFailed(error){throw error;}});
function microphone(t,getUserMedia){
  const original=Object.getOwnPropertyDescriptor(globalThis,"navigator");
  Object.defineProperty(globalThis,"navigator",{configurable:true,value:{mediaDevices:{getUserMedia}}});
  t.after(()=>original?Object.defineProperty(globalThis,"navigator",original):delete globalThis.navigator);
}

test("late microphone consent after session close releases the track without attaching it",async t=>{
  let resolve,attached=false;
  microphone(t,()=>new Promise(done=>{resolve=done;}));
  const state=media();state.voice={sender:{replaceTrack(){attached=true;}}};
  const pending=RemoteMedia.prototype.syncVoice.call(state);
  state.closed=true;state.audioEpoch++;
  const late=stream();resolve(late);await pending;
  assert.equal(late.getTracks()[0].ended,true);assert.equal(attached,false);
});

test("slow hangup cannot stop the microphone belonging to an immediately redialled call",async t=>{
  let finishDetach;const next=stream();microphone(t,async()=>next);
  const state=media(),old=stream();state.microphone=old;state.session.voice.stage="idle";
  state.voice={sender:{replaceTrack(track){return track?Promise.resolve():new Promise(done=>{finishDetach=done;});}}};
  const hangingUp=RemoteMedia.prototype.syncVoice.call(state);
  assert.equal(old.getTracks()[0].ended,true);assert.equal(state.microphone,null);
  state.session.voice={stage:"active",id:"next-call",local_muted:false};
  await RemoteMedia.prototype.syncVoice.call(state);
  finishDetach();await hangingUp;
  assert.equal(state.microphone,next);assert.equal(next.getTracks()[0].ended,false);
});
