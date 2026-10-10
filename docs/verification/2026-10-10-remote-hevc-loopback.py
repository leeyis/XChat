"""Synthetic native-MFT HEVC -> production SCTP/WebCodecs, isolated Chromium.

Only serves the generated synthetic fixture; never reads or captures a screen.
"""
import asyncio
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.request import urlopen

ROOT = Path(__file__).resolve().parents[2]
common = (ROOT / 'docs/verification/2026-10-10-remote-voice-loopback.py').read_text(encoding='utf-8')
namespace = {'__file__': str(ROOT / 'docs/verification/2026-10-10-remote-voice-loopback.py')}
exec(common[:common.index('TEST = r')], namespace)
Page = namespace['Page']
FIXTURE = ROOT / 'analysis/remote-perf-20261010'

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        files = {'/remote-hevc.js': ROOT / 'frontend/src/remote-hevc.js',
                 '/sample.h265': FIXTURE / 'synthetic-production-mft.h265',
                 '/sample.json': FIXTURE / 'hevc-production-mft-report.json',
                 '/forced.xhv': FIXTURE / 'synthetic-production-forced-15.xhv'}
        payload = files[self.path].read_bytes() if self.path in files else b'<!doctype html><title>Synthetic HEVC loopback</title>'
        self.send_response(200)
        self.send_header('Content-Type', 'text/javascript' if self.path.endswith('.js') else 'application/json' if self.path.endswith('.json') else 'application/octet-stream' if self.path in files else 'text/html')
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)
    def log_message(self, *_): pass

TEST = r"""(async()=>{
 const targetFPS=__TARGET_FPS__;
 const {HEVC_CHANNEL,decodeNativeHevcFrame,hevcCodecFromAnnexB,createRemoteHevcSender,createRemoteHevcReceiver}=await import('/remote-hevc.js');
 const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
 const until=async(fn,label)=>{for(let i=0;i<600;i++){if(fn())return;await wait(25);}throw new Error(label);};
 const source=new Uint8Array(await(await fetch('/sample.h265')).arrayBuffer()),report=await(await fetch('/sample.json')).json();
 const realPacket=decodeNativeHevcFrame(await(await fetch('/forced.xhv')).arrayBuffer());
 const packetCodec=hevcCodecFromAnnexB(realPacket.data);
 if(packetCodec!=='hev1.1.6.H120.90'||!realPacket.keyframe||!realPacket.hardwareEncoder)throw new Error('native forced-key packet invalid');
 const a=new RTCPeerConnection({iceServers:[]}),b=new RTCPeerConnection({iceServers:[]});
 let controlA=a.createDataChannel('xchat-control'),controlB,videoChannel;
 const controls=[],errors=[],frames=[];let requestedKey=false,sentPackets=0,sentBytes=0,largestPacket=0,dropSequence=null,droppedPackets=0,holdSequence=null,heldPackets=[],videoTransmit;
 const send=(side,control,message)=>{controls.push({side,action:message.action,streamId:message.streamId,reason:message.reason});if(control?.readyState!=='open')return false;control.send(JSON.stringify(message));};
 const canvas=document.createElement('canvas');canvas.width=1920;canvas.height=1080;document.body.append(canvas);const context=canvas.getContext('2d');
 const receiver=createRemoteHevcReceiver({sessionId:'synthetic',sendControl:message=>send('receiver',controlB,message),onFallback:reason=>errors.push({side:'receiver',reason}),
   onFrame:(frame,metadata)=>{context.drawImage(frame,0,0);frames.push({timestamp:frame.timestamp,revision:metadata.revision,width:frame.displayWidth,height:frame.displayHeight,decodeMs:metadata.decodeMs,receiveToDecodeMs:metadata.receiveToDecodeMs});}});
 const create=a.createDataChannel.bind(a);
 a.createDataChannel=(label,config)=>{const channel=create(label,config);if(label===HEVC_CHANNEL){videoChannel=channel;const send=channel.send.bind(channel);
 videoTransmit=bytes=>{sentPackets++;sentBytes+=bytes.byteLength;largestPacket=Math.max(largestPacket,bytes.byteLength);send(bytes);};channel.send=bytes=>{
   const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);
   if(view.getBigUint64(16,true)===dropSequence&&view.getUint16(40,true)===1){dropSequence=null;droppedPackets++;return;}
   if(view.getBigUint64(16,true)===holdSequence){heldPackets.push(bytes.slice());return;}
   videoTransmit(bytes);
 };}return channel;};
 const sender=createRemoteHevcSender({pc:a,sessionId:'synthetic',sendControl:message=>send('sender',controlA,message),requestKeyFrame:()=>{requestedKey=true;},
   onFallback:reason=>errors.push({side:'sender',reason}),limits:{packetBytes:1200}});
 controlA.onmessage=event=>sender.handleControl(event.data);
 b.ondatachannel=event=>{if(receiver.bindChannel(event.channel))return;controlB=event.channel;controlB.onmessage=e=>receiver.handleControl(e.data);};
 const name=new TextEncoder().encode(report.encoder);
 function packet(sample,data,sequence,timestampUs){
   const header=68+name.length,bytes=new Uint8Array(header+data.length),view=new DataView(bytes.buffer);
   for(const [offset,value]of [[0,0x31564858],[4,header],[8,1920],[12,1080],[16,1920],[20,1080],[40,0],[44,sample.encode_us],[48,2+(sample.keyframe?1:0)],[52,data.length],[60,targetFPS],[64,sample.encode_us]])view.setUint32(offset,value,true);
   view.setBigUint64(24,BigInt(sequence),true);view.setBigUint64(32,BigInt(timestampUs),true);view.setUint16(56,name.length,true);bytes.set(name,68);bytes.set(data,header);return bytes;
 }
 try {
   await a.setLocalDescription(await a.createOffer());await until(()=>a.iceGatheringState==='complete','offer ICE');
   await b.setRemoteDescription(a.localDescription);await b.setLocalDescription(await b.createAnswer());await until(()=>b.iceGatheringState==='complete','answer ICE');
   await a.setRemoteDescription(b.localDescription);await until(()=>a.connectionState==='connected'&&b.connectionState==='connected'&&controlA.readyState==='open'&&controlB?.readyState==='open','connect');
   const supported=await receiver.probe();await until(()=>sender.supported||sender.failed,'HEVC capability');
   if(!supported||sender.failed)throw new Error(JSON.stringify(errors));
   let offset=0,accepted=0;const started=performance.now();sender.stats();receiver.stats();
   for(let i=0;i<report.frames.length;i++){
     const sample=report.frames[i],data=source.subarray(offset,offset+sample.bytes);offset+=sample.bytes;
     if(!await sender.sendFrame(packet(sample,data,i+1,Math.round(i*1000000/targetFPS)),{revision:1}))throw new Error(`frame ${i} not sent ${JSON.stringify({errors,controls,decoded:frames.length,sender:sender.stats(),receiver:receiver.stats()})}`);
     accepted++;await wait(1000/targetFPS);
   }
   await until(()=>frames.length===accepted||errors.length>0,`decoded ${frames.length}/${accepted}`);
   if(errors.length)throw new Error(JSON.stringify(errors));
   const elapsed=performance.now()-started,cleanFrames=frames.length,cleanSender=sender.stats(),cleanReceiver=receiver.stats();
   if(frames.some(frame=>frame.width!==1920||frame.height!==1080))throw new Error('resolution changed');
   if(!(sentPackets>accepted&&largestPacket<=1200))throw new Error('fragmentation was not exercised');
   let sampleOffset=0;const samples=report.frames.map(sample=>{const data=source.subarray(sampleOffset,sampleOffset+sample.bytes);sampleOffset+=sample.bytes;return {sample,data};});
   const sendSample=async(index,sequence,revision=1)=>{const {sample,data}=samples[index];return sender.sendFrame(packet(sample,data,sequence,sequence*33333),{revision});};
   if(!await sendSample(0,61))throw new Error('recovery phase key send failed');await until(()=>frames.length===cleanFrames+1,'recovery phase first key');
   dropSequence=62n;await sendSample(1,62);await wait(33);await sendSample(2,63);
   await until(()=>requestedKey,'missing fragment must request IDR');
   if(droppedPackets!==1)throw new Error('loss injection did not drop exactly one fragment');
   if(!await sendSample(15,64))throw new Error('forced IDR recovery send failed');await wait(33);if(!await sendSample(16,65))throw new Error('post-recovery delta send failed');
   await until(()=>frames.length===cleanFrames+3,'decode must recover from the independently decodable IDR');
   if(errors.length)throw new Error(JSON.stringify(errors));
   const recoveryStats=receiver.stats(),epochsBefore=controls.filter(message=>message.action==='config').map(message=>message.streamId);
   await wait(2100);
   if(!await sendSample(0,1))throw new Error('native idle restart key failed');
   if(!await sendSample(1,2))throw new Error('native idle restart delta failed');
   await until(()=>frames.length===cleanFrames+5,'native sequence/PTS reset must reopen the decoder');
   const epochsAfter=controls.filter(message=>message.action==='config').map(message=>message.streamId);
   if(epochsAfter.length!==epochsBefore.length+1||epochsAfter.at(-1)===epochsBefore.at(-1))throw new Error('native reset reused old epoch');
   const beforePause=frames.length;holdSequence=3n;await sendSample(2,3);
   if(!heldPackets.length)throw new Error('pause test did not retain in-flight fragments');
   receiver.reset({revision:2,paused:true});sender.reset();holdSequence=null;
   for(const bytes of heldPackets)videoTransmit(bytes);await wait(100);
   if(frames.length!==beforePause||receiver.ready||sender.failed)throw new Error('paused revision presented stale pixels');
   receiver.reset({revision:3,paused:false});await wait(50);
   if(!await sendSample(0,4,3))throw new Error('resume new revision key failed');
   if(frames.length!==beforePause+1||frames.at(-1).revision!==3)throw new Error('resume failed to present new revision');
   return {result:'passed',syntheticOnly:true,pcCount:2,codec:packetCodec,actualNativePacket:{nativeMs:realPacket.nativeMs,captureMs:realPacket.captureMs,encodeMs:realPacket.encodeMs,encoder:realPacket.encoderImplementation},
     dataChannel:{ordered:videoChannel.ordered,maxRetransmits:videoChannel.maxRetransmits,maxMessageSize:a.sctp.maxMessageSize,packetLimit:1200,largestPacket,sentPackets,sentBytes},
     targetFPS,accepted,decoded:cleanFrames,elapsedMs:elapsed,observedSyntheticFps:cleanFrames*1000/elapsed,requestedKey,controls,errors,
     sender:cleanSender,receiver:cleanReceiver,recovery:{intentionallyDroppedFragments:droppedPackets,decodedAfterLoss:2,receiver:recoveryStats},
     idleReset:{idleMs:2100,nativeSequenceRestart:1,nativeTimestampRestart:33333,decodedAfterReset:2,epochs:epochsAfter},
     pauseResume:{delayedOldFragments:heldPackets.length,staleFramesPresented:0,resumedRevision:frames.at(-1).revision},frames};
 }finally{sender.close();receiver.close();a.close();b.close();}
})()"""

async def run(profile, browser, port):
    active = profile / 'DevToolsActivePort'
    for _ in range(200):
        if active.exists(): break
        await asyncio.sleep(.1)
    cdp = active.read_text().splitlines()[0]
    targets = json.load(urlopen(f'http://127.0.0.1:{cdp}/json/list', timeout=3))
    page = await Page().connect(next(t for t in targets if t['type'] == 'page'))
    try:
        await page.call('Runtime.enable')
        await page.call('Page.navigate', {'url': f'http://127.0.0.1:{port}/'})
        await asyncio.sleep(.3)
        target_fps = int(sys.argv[1]) if len(sys.argv)>1 else 30
        assert target_fps in [30,60]
        report = await page.js(TEST.replace('__TARGET_FPS__',str(target_fps)))
        report['browser'] = await page.call('Browser.getVersion')
        report['runtime_exceptions'] = page.errors
        assert not page.errors, page.errors
        suffix = '' if target_fps==30 else '-60'
        (ROOT / f'docs/verification/2026-10-10-remote-hevc-loopback{suffix}.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print(json.dumps({key: report.get(key) for key in ['result','codec','accepted','decoded','observedSyntheticFps','dataChannel','receiver']}),flush=True)
    finally:
        await page.call('Browser.close')
        await page.ws.close()

if __name__ == '__main__':
    server = ThreadingHTTPServer(('127.0.0.1',0),Handler)
    threading.Thread(target=server.serve_forever,daemon=True).start()
    profile = Path(tempfile.mkdtemp(prefix='xchat-hevc-loopback-'))
    chrome = Path(os.environ.get('PROGRAMFILES','C:/Program Files'))/'Google/Chrome/Application/chrome.exe'
    browser = subprocess.Popen([str(chrome),'--headless=new','--remote-debugging-port=0',f'--user-data-dir={profile}',
        '--no-first-run','--no-default-browser-check','--autoplay-policy=no-user-gesture-required','about:blank'],
        stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,creationflags=subprocess.CREATE_NO_WINDOW)
    try: asyncio.run(run(profile,browser,server.server_port))
    finally:
        server.shutdown()
        if browser.poll() is None: browser.terminate()
        try: browser.wait(timeout=2)
        except subprocess.TimeoutExpired:
            subprocess.run(['taskkill','/F','/T','/PID',str(browser.pid)],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
