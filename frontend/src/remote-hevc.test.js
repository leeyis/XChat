import test from "node:test";
import assert from "node:assert/strict";
import {HEVC_CHANNEL,decodeNativeHevcFrame,hevcCodecFromAnnexB,createRemoteHevcSender,createRemoteHevcReceiver} from "./remote-hevc.js";
import {createHevcControlOutbox} from "./remote-hevc-media.js";

const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const until=async predicate=>{for(let i=0;i<100;i++){if(predicate())return;await wait(5);}assert.fail("condition timed out");};
const nal=(type,payload=[])=>[0,0,0,1,type<<1,1,...payload];
const keyData=()=>new Uint8Array([
  ...nal(32,[1]),...nal(33,[1,0x21,0x60,0,0,3,0,0x90,0,0,3,0,0,3,0,0x78]),...nal(34,[1]),...nal(19,new Array(2400).fill(7)),
]);
function nativePacket({data=keyData(),flags=3,sequence=1,fixed=68}={}) {
  const name=new TextEncoder().encode("Synthetic hardware test encoder"),header=fixed+name.length;
  const bytes=new Uint8Array(header+data.length),view=new DataView(bytes.buffer);
  view.setUint32(0,0x31564858,true);view.setUint32(4,header,true);
  for(const [offset,value]of [[8,1920],[12,1080],[16,2560],[20,1440],[40,1250],[44,8250],[48,flags],[52,data.length],[60,30]])view.setUint32(offset,value,true);
  view.setBigUint64(24,BigInt(sequence),true);view.setBigUint64(32,BigInt(sequence*33333),true);view.setUint16(56,name.length,true);
  if(fixed>=68)view.setUint32(64,11250,true);bytes.set(name,fixed);bytes.set(data,header);return bytes;
}

class Channel extends EventTarget {
  constructor(){super();this.label=HEVC_CHANNEL;this.readyState="open";this.ordered=false;this.maxRetransmits=0;this.bufferedAmount=0;this.packets=[];}
  send(data){const copy=new Uint8Array(data).slice();this.packets.push(copy);this.forward?.(copy);}
  close(){if(this.readyState==="closed")return;this.readyState="closed";this.dispatchEvent(new Event("close"));}
  deliver(bytes){this.dispatchEvent(new MessageEvent("message",{data:bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength)}));}
}
class Chunk {constructor(value){Object.assign(this,value);}}
class Decoder {
  static configs=[];
  static async isConfigSupported(config){this.configs.push(config);return {supported:true,config};}
  constructor(callbacks){this.callbacks=callbacks;this.decodeQueueSize=0;this.state="unconfigured";this.closedFrames=0;}
  configure(config){assert.equal(config.description,undefined);assert.equal(config.hardwareAcceleration,"prefer-hardware");this.config=config;this.state="configured";}
  decode(chunk){
    this.decodeQueueSize++;queueMicrotask(()=>{this.decodeQueueSize--;if(this.state==="closed")return;
      this.callbacks.output({timestamp:chunk.timestamp,displayWidth:1920,displayHeight:1080,close:()=>this.closedFrames++});
    });
  }
  close(){this.state="closed";}
}
function pair(options={}) {
  const sending=new Channel(),receiving=new Channel(),frames=[],fallbacks=[],keyRequests=[],controls=[];
  let sender,receiver;
  sending.forward=packet=>queueMicrotask(()=>receiving.deliver(packet));
  receiver=createRemoteHevcReceiver({sessionId:"session",VideoDecoder:options.Decoder??Decoder,EncodedVideoChunk:Chunk,
    sendControl:msg=>{controls.push(msg);return options.controlOpen===false?false:queueMicrotask(()=>sender.handleControl(msg));},onFrame:(frame,metadata)=>{frames.push(metadata);options.onFrame?.(frame);},
    onFallback:reason=>fallbacks.push(reason),limits:{frameTimeoutMs:25,...options.receiverLimits}});
  receiver.bindChannel(receiving);
  sender=createRemoteHevcSender({sessionId:"session",pc:{sctp:{maxMessageSize:1200},createDataChannel:(label,config)=>{assert.equal(label,HEVC_CHANNEL);assert.deepEqual(config,{ordered:false,maxRetransmits:0});return sending;}},
    sendControl:msg=>options.controlOpen===false?false:queueMicrotask(()=>receiver.handleControl(msg)),requestKeyFrame:reason=>keyRequests.push(reason),onFallback:reason=>fallbacks.push(reason),
    limits:{packetBytes:1100,configTimeoutMs:100,...options.senderLimits}});
  return {sender,receiver,sending,receiving,frames,fallbacks,keyRequests,controls,close:()=>{sender.close();receiver.close();}};
}

test("native HEVC header validates bounds and retains actual hardware/capture timings",()=>{
  const frame=decodeNativeHevcFrame(nativePacket());
  assert.equal(frame.nativeMs,11.25);assert.equal(frame.captureMs,1.25);assert.equal(frame.encodeMs,8.25);
  assert.equal(frame.hardwareEncoder,true);assert.equal(frame.sourceWidth,2560);assert.equal(frame.width,1920);
  assert.equal(frame.timestampUs,33333);assert.equal(frame.sequence,"1");
  assert.equal(decodeNativeHevcFrame(nativePacket({fixed:64})).nativeMs,null);
  assert.equal(decodeNativeHevcFrame(nativePacket({data:new Uint8Array(),flags:6})).unchanged,true);
  assert.throws(()=>decodeNativeHevcFrame(nativePacket().subarray(0,70)),/参数无效/);
  assert.throws(()=>decodeNativeHevcFrame(nativePacket({flags:4})),/参数无效/);
  const overflow=nativePacket();new DataView(overflow.buffer).setBigUint64(32,2n**63n,true);
  assert.throws(()=>decodeNativeHevcFrame(overflow),/参数无效/);
});

test("SPS derives actual High-tier HEVC codec with emulation prevention removed",()=>{
  assert.equal(hevcCodecFromAnnexB(keyData()),"hev1.1.6.H120.90");
  assert.equal(hevcCodecFromAnnexB(new Uint8Array(nal(1,[1,2,3]))),null);
});

test("reliable config ACK precedes fragmented video and decode output is always closed",async()=>{
  let closed=0;
  const p=pair({onFrame:frame=>{const original=frame.close;frame.close=()=>{closed++;original();};}});
  try {
    assert.equal(await p.receiver.probe(),true);await until(()=>p.sender.supported);
    assert.equal(p.sender.ready,false);assert.equal(await p.sender.sendFrame(nativePacket(),{revision:4}),true);
    await until(()=>p.frames.length===1);
    assert.ok(p.sending.packets.length>=3);assert.ok(p.sending.packets.every(bytes=>bytes.length<=1100));
    assert.equal(p.sender.ready,true);assert.equal(closed,1);assert.equal(p.frames[0].codecString,"hev1.1.6.H120.90");
    assert.equal(p.receiver.stats().hardwareDecoder,null);assert.equal(p.receiver.stats().lossPercent,null);
    assert.equal(p.receiver.stats().hardwareEncoder,true);assert.equal(p.sender.stats().framesSent,1);
    assert.deepEqual(p.fallbacks,[]);
  }finally{p.close();}
});

test("HEVC config waits for a recovered reliable channel while established video survives later control gaps",async()=>{
  const sending=new Channel(),receiving=new Channel(),frames=[],fallbacks=[],acknowledgements=[];
  let sender,receiver;
  const host={session:{id:'session'},channel:{readyState:'closed',bufferedAmount:0,
    send:value=>queueMicrotask(()=>receiver.handleControl(JSON.parse(value)))}};
  const viewer={session:{id:'session'},channel:{readyState:'closed',bufferedAmount:0,
    send:value=>{const message=JSON.parse(value);acknowledgements.push(message);queueMicrotask(()=>sender.handleControl(message));}}};
  const hostControls=createHevcControlOutbox(host),viewerControls=createHevcControlOutbox(viewer);
  sending.forward=packet=>queueMicrotask(()=>receiving.deliver(packet));
  receiver=createRemoteHevcReceiver({sessionId:'session',VideoDecoder:Decoder,EncodedVideoChunk:Chunk,
    sendControl:viewerControls.send,onFrame:(_,metadata)=>frames.push(metadata),onFallback:reason=>fallbacks.push(reason)});
  receiver.bindChannel(receiving);
  sender=createRemoteHevcSender({sessionId:'session',pc:{sctp:{maxMessageSize:1200},createDataChannel:()=>sending},
    sendControl:hostControls.send,onFallback:reason=>fallbacks.push(reason),limits:{configTimeoutMs:300}});
  try {
    sender.handleControl({type:'xchat-hevc',version:1,sessionId:'session',action:'capability',supported:true});
    const first=sender.sendFrame(nativePacket());await wait(15);
    assert.equal(sender.ready,false);assert.equal(sending.packets.length,0);
    host.channel.readyState=viewer.channel.readyState='open';hostControls.flush();viewerControls.flush();
    assert.equal(await first,true);assert.equal(frames.length,1);
    host.channel.readyState=viewer.channel.readyState='closed';
    for(let sequence=2;sequence<=7;sequence++){
      assert.equal(await sender.sendFrame(nativePacket({data:new Uint8Array(nal(1,[1,2,3])),flags:2,sequence})),true);
      await until(()=>frames.length===sequence);
    }
    await wait(230);
    assert.equal(sender.ready,true);assert.equal(receiver.ready,true);assert.equal(frames.length,7);
    assert.equal(acknowledgements.filter(message=>message.action==='frame-ack').length,1);
    host.channel.readyState=viewer.channel.readyState='open';hostControls.flush();viewerControls.flush();await wait(5);
    assert.equal(acknowledgements.filter(message=>message.action==='frame-ack').at(-1).sequence,'7');
    assert.equal(await sender.sendFrame(nativePacket({sequence:8})),true);await until(()=>frames.length===8);
    assert.deepEqual(fallbacks,[]);
  }finally{sender.close();receiver.close();hostControls.close();viewerControls.close();}
});

test("reordered and duplicated fragments decode a single complete access unit",async()=>{
  const p=pair(),held=[];p.sending.forward=packet=>held.push(packet);
  try {
    await p.receiver.probe();await until(()=>p.sender.supported);const sent=p.sender.sendFrame(nativePacket());await until(()=>held.length>=3);
    p.receiving.deliver(held.at(-1));p.receiving.deliver(held.at(-1));
    for(const packet of held.slice(0,-1).reverse())p.receiving.deliver(packet);
    assert.equal(await sent,true);await until(()=>p.frames.length===1);assert.equal(p.receiver.stats().duplicatePackets,1);
    for(const packet of held)p.receiving.deliver(packet);await wait(10);assert.equal(p.frames.length,1);
  }finally{p.close();}
});

test("missing fragments expire, delta references are discarded, and a new IDR recovers",async()=>{
  const p=pair();
  try {
    await p.receiver.probe();await until(()=>p.sender.supported);await p.sender.sendFrame(nativePacket());await until(()=>p.frames.length===1);
    let dropped=false;
    p.sending.forward=packet=>{if(new DataView(packet.buffer).getUint16(40,true)===1&&!dropped){dropped=true;return;}queueMicrotask(()=>p.receiving.deliver(packet));};
    const delta=new Uint8Array(nal(1,new Array(2400).fill(2)));
    await p.sender.sendFrame(nativePacket({data:delta,flags:2,sequence:2}));
    await p.sender.sendFrame(nativePacket({data:delta,flags:2,sequence:3}));
    await until(()=>p.keyRequests.length>0);assert.equal(p.frames.length,1);
    assert.equal(await p.sender.sendFrame(nativePacket({data:delta,flags:2,sequence:4})),false);
    assert.equal(await p.sender.sendFrame(nativePacket({sequence:5})),true);
    await wait(50);assert.equal(p.frames.length,2,JSON.stringify({sender:p.sender.stats(),receiver:p.receiver.stats(),fallbacks:p.fallbacks,requests:p.keyRequests}));
    assert.ok(p.receiver.stats().framesDropped>=1);assert.deepEqual(p.fallbacks,[]);
  }finally{p.close();}
});

test("sender applies whole-frame backpressure without sending partial stale video",async()=>{
  const p=pair({senderLimits:{maxBufferedAmount:4096}});
  try {
    await p.receiver.probe();await until(()=>p.sender.supported);p.sending.bufferedAmount=4000;
    assert.equal(await p.sender.sendFrame(nativePacket()),false);assert.equal(p.sending.packets.length,0);assert.equal(p.sender.ready,false);
    p.sending.bufferedAmount=0;assert.equal(await p.sender.sendFrame(nativePacket({sequence:2})),true);
    await until(()=>p.frames.length===1);assert.equal(p.sender.stats().framesDropped,1);
  }finally{p.close();}
});

test("unsupported HEVC and missing config acknowledgements give explicit RTP fallback",async()=>{
  class Unsupported extends Decoder {static async isConfigSupported(){return {supported:false};}}
  const p=pair({Decoder:Unsupported});
  try {assert.equal(await p.receiver.probe(),false);await until(()=>p.sender.failed);assert.equal(p.sender.ready,false);assert.equal(p.fallbacks.length,1);}finally{p.close();}
  const q=pair({senderLimits:{configTimeoutMs:10}});
  try {
    q.sender.handleControl({type:"xchat-hevc",version:1,sessionId:"session",action:"capability",supported:true});
    q.receiver.close();assert.equal(await q.sender.sendFrame(nativePacket()),false);assert.equal(q.sender.failed,true);
    assert.match(q.fallbacks[0],/配置未获确认/);
  }finally{q.close();}
});

test("foreign sessions and old epoch fragments cannot alter the new stream",async()=>{
  const p=pair();
  try {
    assert.equal(p.sender.handleControl({type:"xchat-hevc",version:1,sessionId:"other",action:"capability",supported:true}),false);
    assert.equal(p.sender.supported,false);await p.receiver.probe();await until(()=>p.sender.supported);
    await p.sender.sendFrame(nativePacket(),{revision:1});await until(()=>p.frames.length===1);const old=p.sending.packets.map(bytes=>bytes.slice());
    p.sender.reset();await p.sender.sendFrame(nativePacket({sequence:2}),{revision:2});await until(()=>p.frames.length===2);
    for(const bytes of old)p.receiving.deliver(bytes);await wait(10);assert.equal(p.frames.length,2);
  }finally{p.close();}
});

test("a native encoder restart reconfigures the decoder even when dimensions and revision match",async()=>{
  const p=pair();
  try {
    await p.receiver.probe();await until(()=>p.sender.supported);
    await p.sender.sendFrame(nativePacket({sequence:20}),{revision:1});await until(()=>p.frames.length===1);
    const previous=new DataView(p.sending.packets.at(-1).buffer).getBigUint64(8,true);
    await p.sender.sendFrame(nativePacket({sequence:1}),{revision:1});await until(()=>p.frames.length===2);
    const current=new DataView(p.sending.packets.at(-1).buffer).getBigUint64(8,true);
    assert.notEqual(previous,current);assert.equal(p.frames[1].timestampUs,33333);assert.deepEqual(p.fallbacks,[]);
  }finally{p.close();}
});

test("closing while actual codec support is pending prevents late decoder/channel resurrection",async()=>{
  let resolveSupport;
  class Pending extends Decoder {static isConfigSupported(){return new Promise(resolve=>{resolveSupport=resolve;});}}
  const p=pair({Decoder:Pending});
  try {
    p.sender.handleControl({type:"xchat-hevc",version:1,sessionId:"session",action:"capability",supported:true});
    const sent=p.sender.sendFrame(nativePacket());await until(()=>resolveSupport);
    p.close();resolveSupport({supported:true});assert.equal(await sent,false);await wait(10);
    assert.equal(p.sending.packets.length,0);assert.equal(p.receiver.ready,false);assert.equal(p.sender.ready,false);assert.deepEqual(p.fallbacks,[]);
  }finally{p.close();}
});

test("invalid cross-fragment lengths are rejected without decoding mixed payloads",async()=>{
  const p=pair(),held=[];p.sending.forward=packet=>held.push(packet);
  try {
    await p.receiver.probe();await until(()=>p.sender.supported);const sent=p.sender.sendFrame(nativePacket());await until(()=>held.length>=3);
    p.receiving.deliver(held[0]);const invalid=held[1].slice();new DataView(invalid.buffer).setUint32(36,1,true);p.receiving.deliver(invalid);
    p.receiving.deliver(held.at(-1));await wait(60);
    assert.equal(p.frames.length,0);assert.equal(p.receiver.stats().invalidPackets,1);assert.ok(p.receiver.stats().framesDropped>=1);
    p.close();assert.equal(await sent,false);
  }finally{p.close();}
});

test("RTP handoff waits for actual first decoder output rather than configuration ACK",async()=>{
  class SlowStart extends Decoder {decode(chunk){setTimeout(()=>super.decode(chunk),60);}}
  const p=pair({Decoder:SlowStart});
  try {
    await p.receiver.probe();await until(()=>p.sender.supported);
    const sent=p.sender.sendFrame(nativePacket());await wait(20);
    assert.equal(p.receiver.ready,true);assert.equal(p.sender.ready,false);assert.equal(p.frames.length,0);
    assert.equal(await sent,true);assert.equal(p.sender.ready,true);assert.equal(p.frames.length,1);
  }finally{p.close();}
});

test("an entirely lost last desktop frame is recovered without waiting for more screen changes",async()=>{
  const p=pair();
  try {
    await p.receiver.probe();await until(()=>p.sender.supported);await p.sender.sendFrame(nativePacket());
    p.sending.forward=()=>{};
    await p.sender.sendFrame(nativePacket({data:new Uint8Array(nal(1,[1,2,3])),flags:2,sequence:2}));
    await wait(1250);assert.ok(p.keyRequests.includes("decoded-frame-ack-timeout"));assert.equal(p.frames.length,1);
    p.sending.forward=packet=>queueMicrotask(()=>p.receiving.deliver(packet));
    await p.sender.sendFrame(nativePacket({sequence:3}));await until(()=>p.frames.length===2);
  }finally{p.close();}
});

test("pause during codec support/config ACK cancels old revision without failing the pipeline",async()=>{
  let releaseSupport,calls=0;
  class Gate extends Decoder {
    static isConfigSupported(config){calls++;return calls===1?new Promise(resolve=>{releaseSupport=resolve;}):super.isConfigSupported(config);}
  }
  const p=pair({Decoder:Gate});
  try {
    p.sender.handleControl({type:"xchat-hevc",version:1,sessionId:"session",action:"capability",supported:true});
    const pending=p.sender.sendFrame(nativePacket(),{revision:1});await until(()=>releaseSupport);
    p.receiver.reset({revision:2,paused:true});p.sender.reset();releaseSupport({supported:true});
    assert.equal(await pending,false);await wait(10);assert.equal(p.sender.failed,false);assert.equal(p.receiver.failed,false);
    assert.equal(p.sending.packets.length,0);assert.equal(p.frames.length,0);
    p.receiver.handleControl({type:"xchat-hevc",version:1,sessionId:"session",action:"config",streamId:"0000000000000001",revision:1,codec:"hev1.1.6.H120.90",codedWidth:1920,codedHeight:1080});
    await wait(10);assert.equal(calls,1);
    p.receiver.reset({revision:3,paused:false});await wait(10);
    assert.equal(await p.sender.sendFrame(nativePacket({sequence:2}),{revision:3}),true);
    assert.equal(p.frames.length,1);assert.equal(p.frames[0].revision,3);assert.deepEqual(p.fallbacks,[]);
  }finally{p.close();}
});

test("reset disposes a late decoder output without presenting pixels from the paused revision",async()=>{
  let released=0;
  class Late extends Decoder {
    decode(chunk){setTimeout(()=>this.callbacks.output({timestamp:chunk.timestamp,displayWidth:1920,displayHeight:1080,close:()=>released++}),60);}
  }
  const p=pair({Decoder:Late});
  try {
    await p.receiver.probe();await until(()=>p.sender.supported);
    const pending=p.sender.sendFrame(nativePacket(),{revision:1});await until(()=>p.sending.packets.length>=3);
    p.receiver.reset({revision:2,paused:true});p.sender.reset();assert.equal(await pending,false);
    await wait(80);assert.equal(p.frames.length,0);assert.equal(released,1);assert.equal(p.sender.failed,false);assert.equal(p.receiver.ready,false);
  }finally{p.close();}
});

test("reset cancels a channel-open wait and ignores the eventual open event",async()=>{
  const p=pair();
  try {
    p.sending.readyState="connecting";await p.receiver.probe();await until(()=>p.sender.supported);
    const pending=p.sender.sendFrame(nativePacket());await until(()=>p.receiver.ready);
    p.sender.reset();assert.equal(await pending,false);p.sending.readyState="open";p.sending.dispatchEvent(new Event("open"));
    await wait(10);assert.equal(p.sending.packets.length,0);assert.equal(p.sender.failed,false);assert.deepEqual(p.fallbacks,[]);
  }finally{p.close();}
});

test("initial receiver reset retains the expected revision before the control channel opens",async()=>{
  const options={controlOpen:false},p=pair(options);
  try {
    p.receiver.reset({revision:7,paused:false});assert.equal(p.receiver.ready,false);
    options.controlOpen=true;await p.receiver.probe();await until(()=>p.sender.supported);
    assert.equal(await p.sender.sendFrame(nativePacket(),{revision:7}),true);
    assert.equal(p.frames.length,1);assert.equal(p.frames[0].revision,7);assert.deepEqual(p.fallbacks,[]);
  }finally{p.close();}
});

test("a new revision requests its own IDR even immediately after the previous epoch requested one",async()=>{
  const p=pair();
  try {
    await p.receiver.probe();await until(()=>p.sender.supported);
    await p.sender.sendFrame(nativePacket(),{revision:1});
    p.sender.handleControl({type:"xchat-hevc",version:1,sessionId:"session",action:"reset",revision:1,paused:false});
    const previous=p.keyRequests.length;assert.ok(previous>0);
    // A static desktop produces no further frame until force-key is requested.
    p.sender.handleControl({type:"xchat-hevc",version:1,sessionId:"session",action:"reset",revision:2,paused:false});
    assert.equal(p.keyRequests.length,previous+1);
    assert.equal(p.keyRequests.at(-1),"receiver-revision-reset");
    p.receiver.reset({revision:2,paused:false});await wait(0);
    assert.equal(await p.sender.sendFrame(nativePacket({sequence:2}),{revision:2}),true);
    assert.equal(p.frames.at(-1).revision,2);assert.deepEqual(p.fallbacks,[]);
  }finally{p.close();}
});

function holdingDecoder() {
  return class Holding extends Decoder {
    static instance;
    constructor(callbacks){super(callbacks);this.constructor.instance=this;this.held=[];this.submitted=[];this.highWater=0;}
    decode(chunk){
      this.submitted.push(chunk.timestamp);
      if(chunk.type==="key"){super.decode(chunk);return;}
      this.decodeQueueSize++;this.held.push(chunk);this.highWater=Math.max(this.highWater,this.decodeQueueSize);
    }
    release(){
      const chunk=this.held.shift();assert.ok(chunk,"a held decode must exist");this.decodeQueueSize--;
      this.callbacks.output({timestamp:chunk.timestamp,displayWidth:1920,displayHeight:1080,close:()=>this.closedFrames++});
    }
  };
}
const deltaPacket=sequence=>nativePacket({data:new Uint8Array(nal(1,[1,2,3])),flags:2,sequence});
async function startHeldBurst(p){
  await p.receiver.probe();await until(()=>p.sender.supported);assert.equal(await p.sender.sendFrame(nativePacket(),{revision:1}),true);
  for(let sequence=2;sequence<=5;sequence++)assert.equal(await p.sender.sendFrame(deltaPacket(sequence),{revision:1}),true);
}

test("a fourth continuous frame waits within the bounded queue and output alone resumes decode",async()=>{
  const Held=holdingDecoder(),p=pair({Decoder:Held,receiverLimits:{frameTimeoutMs:500}});
  try {
    await startHeldBurst(p);const decoder=Held.instance;
    assert.equal(decoder.held.length,3);assert.equal(decoder.submitted.length,4);
    assert.equal(p.receiver.stats().framesDropped,0);assert.deepEqual(p.keyRequests,[]);
    decoder.release();await until(()=>decoder.submitted.length===5);
    assert.equal(decoder.held.length,3);assert.equal(decoder.highWater,3);
    // No new network packet arrives to trigger the final queued submission.
    while(decoder.held.length)decoder.release();await until(()=>p.frames.length===5);
    assert.deepEqual(p.frames.map(frame=>frame.sequence),["1","2","3","4","5"]);
    assert.equal(p.receiver.stats().framesDropped,0);assert.deepEqual(p.keyRequests,[]);assert.deepEqual(p.fallbacks,[]);
  }finally{p.close();}
});

test("decoder waiting preserves the three-pending-frame limit and requests recovery on overflow",async()=>{
  const Held=holdingDecoder(),p=pair({Decoder:Held,receiverLimits:{frameTimeoutMs:500}});
  try {
    await startHeldBurst(p);
    for(let sequence=6;sequence<=8;sequence++)await p.sender.sendFrame(deltaPacket(sequence),{revision:1});
    const decoder=Held.instance;assert.equal(decoder.submitted.length,4);assert.equal(decoder.highWater,3);
    assert.equal(p.receiver.stats().framesDropped,3);
    while(decoder.held.length)decoder.release();await wait(0);
    assert.equal(decoder.submitted.length,4,"overflow must discard the dependent pending chain");
    await p.sender.sendFrame(nativePacket({sequence:9}),{revision:1});await until(()=>p.frames.length===5);
    assert.equal(p.frames.at(-1).sequence,"9");assert.deepEqual(p.fallbacks,[]);
  }finally{p.close();}
});

test("a complete frame waiting for decoder space still expires at the existing age bound",async()=>{
  const Held=holdingDecoder(),p=pair({Decoder:Held,receiverLimits:{frameTimeoutMs:30}});
  try {
    await startHeldBurst(p);const decoder=Held.instance;
    await wait(75);assert.equal(p.receiver.stats().framesDropped,1);
    decoder.release();await wait(0);assert.equal(decoder.submitted.length,4);
    assert.equal(p.frames.length,2);assert.deepEqual(p.fallbacks,[]);
  }finally{p.close();}
});

for(const action of ["reset","close"])test(`${action} cancels a queued output-driven drain and its trailing ACK`,async()=>{
  const Held=holdingDecoder(),p=pair({Decoder:Held,receiverLimits:{frameTimeoutMs:500}});
  try {
    await startHeldBurst(p);const decoder=Held.instance;decoder.release();
    const displayed=p.frames.length,acknowledgements=p.controls.filter(message=>message.action==="frame-ack").length;
    if(action==="reset")p.receiver.reset({revision:2,paused:true});else p.receiver.close();
    await wait(230);
    assert.equal(decoder.submitted.length,4);assert.equal(p.receiver.ready,false);
    assert.equal(p.controls.filter(message=>message.action==="frame-ack").length,acknowledgements);
    while(decoder.held.length)decoder.release();await wait(0);
    assert.equal(p.frames.length,displayed);assert.deepEqual(p.fallbacks,[]);
  }finally{p.close();}
});

test("synchronous decoder output may reset the receiver without old decode state or ACK escaping",async()=>{
  let once=true,closedFrames=0,p;
  class Synchronous extends Decoder {
    decode(chunk){this.callbacks.output({timestamp:chunk.timestamp,displayWidth:1920,displayHeight:1080,close:()=>closedFrames++});}
  }
  p=pair({Decoder:Synchronous,onFrame:()=>{
    if(once){once=false;p.receiver.reset({revision:2,paused:true});p.sender.reset();}
  }});
  try {
    await p.receiver.probe();await until(()=>p.sender.supported);
    assert.equal(await p.sender.sendFrame(nativePacket(),{revision:1}),false);await wait(0);
    assert.equal(p.receiver.ready,false);assert.equal(closedFrames,1);
    assert.equal(p.controls.filter(message=>message.action==="frame-ack").length,0);
    p.receiver.reset({revision:3,paused:false});await wait(0);
    assert.equal(await p.sender.sendFrame(nativePacket(),{revision:3}),true);
    assert.equal(p.frames.at(-1).revision,3);assert.equal(closedFrames,2);
    assert.equal(p.controls.filter(message=>message.action==="frame-ack").length,1);assert.deepEqual(p.fallbacks,[]);
  }finally{p.close();}
});

test("first decoded output is acknowledged immediately and later outputs coalesce with a static-tail ACK",async()=>{
  const p=pair();
  try {
    await p.receiver.probe();await until(()=>p.sender.supported);assert.equal(await p.sender.sendFrame(nativePacket()),true);
    const acknowledgements=()=>p.controls.filter(message=>message.action==="frame-ack");
    assert.deepEqual(acknowledgements().map(message=>message.sequence),["1"]);
    for(let sequence=2;sequence<=8;sequence++)await p.sender.sendFrame(deltaPacket(sequence));
    await until(()=>p.frames.length===8);assert.equal(acknowledgements().length,1);
    await wait(250);assert.deepEqual(acknowledgements().map(message=>message.sequence),["1","8"]);
    await p.sender.sendFrame(deltaPacket(9));await until(()=>p.frames.length===9);await wait(220);
    assert.deepEqual(acknowledgements().map(message=>message.sequence),["1","8","9"]);
    assert.deepEqual(p.fallbacks,[]);
  }finally{p.close();}
});

test("an older coalesced ACK cannot clear recovery for a completely lost final desktop frame",async()=>{
  const p=pair();
  try {
    await p.receiver.probe();await until(()=>p.sender.supported);await p.sender.sendFrame(nativePacket());
    await p.sender.sendFrame(deltaPacket(2));await until(()=>p.frames.length===2);
    p.sending.forward=()=>{};await p.sender.sendFrame(deltaPacket(3));
    await wait(250);
    assert.equal(p.controls.filter(message=>message.action==="frame-ack").at(-1).sequence,"2");
    await wait(1300);assert.ok(p.keyRequests.includes("decoded-frame-ack-timeout"));
    assert.equal(p.frames.length,2);assert.deepEqual(p.fallbacks,[]);
  }finally{p.close();}
});

test("a new codec epoch immediately acknowledges its first output and cancels the old trailing ACK",async()=>{
  const p=pair();
  try {
    await p.receiver.probe();await until(()=>p.sender.supported);await p.sender.sendFrame(nativePacket(),{revision:1});
    const firstAck=p.controls.find(message=>message.action==="frame-ack");
    await p.sender.sendFrame(deltaPacket(2),{revision:1});await until(()=>p.frames.length===2);
    p.receiver.reset({revision:2,paused:false});p.sender.reset();await wait(0);
    assert.equal(await p.sender.sendFrame(nativePacket(),{revision:2}),true);
    const acks=p.controls.filter(message=>message.action==="frame-ack");
    assert.equal(acks.length,2);assert.notEqual(acks[1].streamId,firstAck.streamId);assert.equal(acks[1].sequence,"1");
    await wait(230);assert.equal(p.controls.filter(message=>message.action==="frame-ack").length,2);
  }finally{p.close();}
});
