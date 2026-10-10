import test from "node:test";
import assert from "node:assert/strict";
import {createNativeFrameReader,decodeNativeFrame,preferRemoteVideoCodecs} from "./remote-capture.js";
import {remoteMediaMetrics} from "./remote-metrics.js";
import {readRemoteTelemetry,remoteTelemetry} from "./remote-telemetry.js";

test("native frame validates payload, exposes real capture backend, and excludes unchanged frames",()=>{
  const buffer=new ArrayBuffer(44),view=new DataView(buffer),bytes=new Uint8Array(buffer);
  bytes.set([88,82,70,49,40,0,1,1]);view.setUint32(8,1,true);view.setUint32(12,1,true);
  view.setBigUint64(16,7n,true);view.setUint32(24,1250,true);bytes.set([1,2,3,255],40);
  const frame=decodeNativeFrame(buffer);assert.equal(frame.captureBackend,"DXGI");assert.equal(frame.captureMs,1.25);
  assert.deepEqual([...frame.pixels],[1,2,3,255]);assert.equal(frame.sequence,"7");
  assert.throws(()=>decodeNativeFrame(buffer.slice(0,42)),/不完整/);
  const idle=buffer.slice(0,40);new DataView(idle).setUint32(36,1,true);
  assert.equal(decodeNativeFrame(idle).unchanged,true);assert.equal(decodeNativeFrame(idle).pixels,null);
  assert.equal(frame.nativeMs,null);
  const timed=new Uint8Array(48);timed.set(bytes.subarray(0,40));timed[4]=44;
  new DataView(timed.buffer).setUint32(40,2500,true);timed.set([1,2,3,255],44);
  assert.equal(decodeNativeFrame(timed).nativeMs,2.5);
  assert.deepEqual([...decodeNativeFrame(timed).pixels],[1,2,3,255]);
});

test("HEVC preference sorts actual receiver capabilities and preserves alternatives",()=>{
  const codec=name=>({mimeType:`video/${name}`,clockRate:90000});
  const caps=codecs=>({getCapabilities:()=>({codecs})});let ordered;
  preferRemoteVideoCodecs({setCodecPreferences:value=>ordered=value},caps([codec("VP8"),codec("H265"),codec("H264"),codec("AV1")]));
  assert.deepEqual(ordered.map(item=>item.mimeType),["video/H265","video/H264","video/VP8","video/AV1"]);
});

test("HEVC preferences retain real decoder levels and profiles without appending encoder variants",()=>{
  const codec=(name,fmtp)=>({mimeType:`video/${name}`,clockRate:90000,...(fmtp?{sdpFmtpLine:fmtp}:{})});
  const sendHevc=codec("H265","level-id=123;profile-id=1;tier-flag=0;tx-mode=SRST");
  const receiveHevc=codec("H265","level-id=180;profile-id=1;tier-flag=0;tx-mode=SRST");
  const main10=codec("H265","level-id=180;profile-id=2;tier-flag=0;tx-mode=SRST");
  const h264=codec("H264","level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42001f");
  const rtx=codec("rtx"),red=codec("red"),fec=codec("ulpfec");
  const receiving=[receiveHevc,main10,h264,rtx,red,fec];
  let ordered;
  preferRemoteVideoCodecs({setCodecPreferences:value=>ordered=value},{getCapabilities:()=>({codecs:receiving})});
  assert.deepEqual(ordered,[receiveHevc,main10,h264,rtx,red,fec]);
  assert.ok(ordered.every(codec=>receiving.some(original=>original===codec)));
  assert.equal(sendHevc.sdpFmtpLine,"level-id=123;profile-id=1;tier-flag=0;tx-mode=SRST");
});

function sharedFixture() {
  const pixels=new Uint8Array(48),view=new DataView(pixels.buffer);
  pixels.set([88,82,70,49,44,0,1,1]);view.setUint32(8,1,true);view.setUint32(12,1,true);view.setBigUint64(16,7n,true);pixels.set([1,2,3,255],44);
  const ack=new Uint8Array(12);ack.set([88,82,83,49]);new DataView(ack.buffer).setBigUint64(4,7n,true);
  const released=[];let listener;
  const webview={addEventListener:(name,fn)=>{assert.equal(name,"sharedbufferreceived");listener=fn;},releaseBuffer:bytes=>released.push(bytes)};
  const emit=(requestId,overrides={})=>{const buffer=pixels.slice().buffer;listener({additionalData:{type:"xchat-frame-v1",requestId,sessionId:"session",revision:"4",sequence:"7",...overrides},getBuffer:()=>buffer});return buffer;};
  return {pixels,ack,webview,released,emit};
}

test("shared-memory frames join a unique request before or after ACK and release after consumption",async()=>{
  const fixture=sharedFixture();let sequence=0;
  const reader=createNativeFrameReader({sessionId:"session",webview:fixture.webview,randomId:()=>`request-${++sequence}`,frame:async(revision,format,requestId)=>{
    assert.equal(revision,4);assert.equal(format,"rgba-shared-v1");
    if(sequence===1)fixture.emit(requestId);else setImmediate(()=>fixture.emit(requestId));
    return fixture.ack;
  }});
  for(let index=0;index<2;index++){
    const received=await reader.read(4);assert.equal(received.transport,"shared-memory");
    assert.deepEqual([...decodeNativeFrame(received.bytes).pixels],[1,2,3,255]);
    assert.equal(fixture.released.length,index);received.release();received.release();
    assert.equal(fixture.released.length,index+1);
  }
  reader.close();
});

test("closed or timed-out requests release late shared events and never reuse them",async()=>{
  const fixture=sharedFixture();let requestId;
  const reader=createNativeFrameReader({sessionId:"session",webview:fixture.webview,timeoutMs:10,frame:async(_,__,id)=>{requestId=id;return fixture.ack;}});
  await assert.rejects(reader.read(4),/超时/);
  const late=fixture.emit(requestId);assert.deepEqual(fixture.released,[late]);
  const next=reader.read(4);reader.close();await assert.rejects(next,/已关闭/);
  const closed=fixture.emit(requestId);assert.deepEqual(fixture.released,[late,closed]);
  await assert.rejects(reader.read(4),/已关闭/);
});

test("shared events with stale session/revision are released, and invoke errors cannot fall back",async()=>{
  const fixture=sharedFixture();let calls=0;
  const reader=createNativeFrameReader({sessionId:"session",webview:fixture.webview,frame:async(_,__,id)=>{
    calls++;fixture.emit(id,{sessionId:"old"});fixture.emit(id,{revision:"3"});fixture.emit(id);
    throw new Error("remote_state_changed: revoked");
  }});
  await assert.rejects(reader.read(4),/revoked/);assert.equal(calls,1);assert.equal(fixture.released.length,3);reader.close();
});

test("shared ACK sequence mismatch and duplicate events release every mapping",async()=>{
  const fixture=sharedFixture();
  const reader=createNativeFrameReader({sessionId:"session",webview:fixture.webview,frame:async(_,__,id)=>{
    fixture.emit(id);fixture.emit(id);const ack=fixture.ack.slice();new DataView(ack.buffer).setBigUint64(4,8n,true);return ack;
  }});
  await assert.rejects(reader.read(4),/序号不匹配/);assert.equal(fixture.released.length,2);reader.close();
});

test("native API-unavailable raw response selects the compatible IPC transport",async()=>{
  const fixture=sharedFixture(),formats=[];
  const reader=createNativeFrameReader({sessionId:"session",webview:fixture.webview,frame:async(_,format)=>{formats.push(format);return fixture.pixels;}});
  for(let index=0;index<2;index++){const received=await reader.read(4);assert.equal(received.transport,"ipc");received.release();}
  assert.deepEqual(formats,["rgba-shared-v1","rgba-v1"]);assert.equal(fixture.released.length,0);reader.close();
});

test("Safari counter fallback reports delivered FPS and never invents hardware or end-to-end delay",()=>{
  const prior={id:"video",timestamp:1000,framesDecoded:10,bytesReceived:1000,packetsReceived:10,packetsLost:0,totalDecodeTime:0.1,jitterBufferDelay:0.2,jitterBufferEmittedCount:10};
  const video={...prior,type:"inbound-rtp",kind:"video",timestamp:3000,framesDecoded:70,bytesReceived:101000,packetsReceived:108,packetsLost:2,totalDecodeTime:0.22,jitterBufferDelay:0.8,jitterBufferEmittedCount:70,codecId:"codec",transportId:"transport"};
  const reports=new Map([["video",video],["codec",{mimeType:"video/H265"}],["transport",{selectedCandidatePairId:"pair",dtlsState:"connected",srtpCipher:"AEAD_AES_128_GCM"}],["pair",{currentRoundTripTime:0.055,localCandidateId:"local",remoteCandidateId:"remote"}],["local",{protocol:"udp",candidateType:"host"}],["remote",{protocol:"udp",candidateType:"host"}]]);
  const {info}=remoteMediaMetrics(reports,false,prior);
  assert.equal(info.fps,30);assert.equal(info.kbps,400);assert.equal(info.rtt,55);assert.equal(info.lossPercent,2);
  assert.equal(info.decodeMs,2);assert.ok(Math.abs(info.jitterBufferMs-10)<0.001);
  assert.equal(info.codec,"H265");assert.equal(info.path,"direct");assert.equal(info.protocol,"udp");
  assert.equal(info.powerEfficientDecoder,null);assert.equal(info.processingMs,null);
  assert.equal(remoteMediaMetrics(reports,false,null).info.fps,null);
});

test("host diagnostics are bounded and cannot replace measured receiver metrics",()=>{
  const message=remoteTelemetry({captureBackend:"DXGI",captureMs:4,sourceWidth:1920,sourceHeight:1080,codec:"H265",powerEfficientEncoder:false,fps:999,rtt:0});
  assert.deepEqual(readRemoteTelemetry(message),{hostCaptureBackend:"DXGI",hostCaptureMs:4,hostSourceWidth:1920,hostSourceHeight:1080,hostCodec:"H265",hostPowerEfficientEncoder:false});
  assert.equal(readRemoteTelemetry("x".repeat(2049)),null);
  assert.equal(readRemoteTelemetry('{"type":"xchat-metrics","version":2,"metrics":{}}'),null);
  assert.deepEqual(readRemoteTelemetry('{"type":"xchat-metrics","version":1,"metrics":{"captureMs":-1,"fps":999,"sourceWidth":999999,"powerEfficientEncoder":"yes"}}'),{});
  const system=remoteTelemetry({},{os:"Windows",os_version:"11",architecture:"x86_64",client_version:"0.1.13",cpu:"CPU",memory_bytes:17179869184,hostname:"private",ip:"private"});
  assert.deepEqual(readRemoteTelemetry(system).hostInfo,{os:"Windows",os_version:"11",architecture:"x86_64",client_version:"0.1.13",cpu:"CPU",memory_bytes:17179869184});
  assert.equal(readRemoteTelemetry(remoteTelemetry({}, {cpu:"x".repeat(257),memory_bytes:Infinity})).hostInfo,undefined);
});
