"""Production worker sender, synthetic native WebSocket, real Edge SCTP/WebCodecs.

No real screen capture, Tauri window, or microphone is used. Native encoder
output is replayed from the independently verified synthetic hardware fixture.
"""
import asyncio
import json
import os
from pathlib import Path
import struct
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.request import urlopen
import websockets

ROOT = Path(__file__).resolve().parents[2]
common = (ROOT/'docs/verification/2026-10-10-remote-voice-loopback.py').read_text(encoding='utf-8')
namespace = {'__file__':str(ROOT/'docs/verification/2026-10-10-remote-voice-loopback.py')}
exec(common[:common.index('TEST = r')], namespace)
Page = namespace['Page']
FIXTURE = ROOT/'analysis/remote-perf-20261010'
report = json.loads((FIXTURE/'hevc-production-mft-report.json').read_text())
encoded = (FIXTURE/'synthetic-production-mft.h265').read_bytes()
samples=[]
offset=0
for sample in report['frames']:
    samples.append((sample,encoded[offset:offset+sample['bytes']]))
    offset+=sample['bytes']
native_reads=[]
tokens=set()

def packet(index,sequence):
    sample,data=samples[index]
    name=report['encoder'].encode()
    value=bytearray(68+len(name)+len(data))
    for pos,n in [(0,0x31564858),(4,68+len(name)),(8,1920),(12,1080),(16,1920),(20,1080),
                  (40,0),(44,sample['encode_us']),(48,2+int(sample['keyframe'])),(52,len(data)),(60,30),(64,33333)]:
        struct.pack_into('<I',value,pos,n)
    struct.pack_into('<Q',value,24,sequence)
    struct.pack_into('<Q',value,32,sequence*33333)
    struct.pack_into('<H',value,56,len(name))
    value[68:68+len(name)]=name
    value[68+len(name):]=data
    return bytes(value)

async def native_socket(socket):
    try:
        auth=json.loads(await asyncio.wait_for(socket.recv(),3))
        token=auth.get('token','')
        assert auth['type']=='auth' and len(token)==64 and token not in tokens
        tokens.add(token)
        revision=int(token[:8],16)
        await socket.send(json.dumps({'type':'ready','format':'hevc-v1','revision':revision}))
        sequence=0
        index=0
        deadline=time.perf_counter()
        async for raw in socket:
            message=json.loads(raw)
            if message['type']=='stop': return
            assert message['type']=='next'
            if message.get('request_keyframe'): index=0
            # Match native deadline pacing. Sleeping a fresh 33ms after each
            # response would add Windows timer/roundtrip overhead every frame.
            deadline=max(deadline+1/30,time.perf_counter())
            await asyncio.sleep(max(0,deadline-time.perf_counter()))
            native_reads.append({'at':time.time()*1000,'revision':revision,'keyRequested':bool(message.get('request_keyframe'))})
            sequence+=1
            await socket.send(packet(index,sequence))
            index=(index+1)%len(samples)
    except websockets.ConnectionClosed:
        pass

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path in ['/remote-hevc.js','/remote-hevc-worker.js','/remote-hevc-worker-client.js']:
            payload=(ROOT/'frontend/src'/self.path[1:]).read_bytes()
            mime='text/javascript'
        else:
            payload=b'<!doctype html><title>Edge worker HEVC synthetic loopback</title>'
            mime='text/html'
        self.send_response(200)
        self.send_header('Content-Type',mime)
        self.send_header('Content-Length',str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)
    def log_message(self,*_): pass

TEST=r"""(async()=>{
const {createRemoteHevcWorkerSender}=await import('/remote-hevc-worker-client.js');
const {createRemoteHevcReceiver}=await import('/remote-hevc.js');
const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const until=async(fn,label)=>{for(let i=0;i<600;i++){if(fn())return;await wait(25);}throw new Error(label+JSON.stringify({errors,stats:sender.stats(),controls}));};
const a=new RTCPeerConnection({iceServers:[]}),b=new RTCPeerConnection({iceServers:[]});
const controlA=a.createDataChannel('xchat-control');let controlB;
const frames=[],errors=[],controls=[];let authorizations=0,captureCalls=0;
const canvas=document.createElement('canvas');canvas.width=1920;canvas.height=1080;document.body.append(canvas);
const context=canvas.getContext('2d',{alpha:false,desynchronized:true});
const send=(channel,value)=>{controls.push(value.action);if(channel?.readyState!=='open')return false;channel.send(JSON.stringify(value));return true;};
const receiver=createRemoteHevcReceiver({sessionId:'worker-synthetic',sendControl:value=>send(controlB,value),
  onFrame:(frame,metadata)=>{context.drawImage(frame,0,0);frames.push({at:performance.now(),revision:metadata.revision,width:frame.displayWidth,height:frame.displayHeight});},
  onFallback:reason=>errors.push({side:'receiver',reason})});
const sender=createRemoteHevcWorkerSender({pc:a,sessionId:'worker-synthetic',sendControl:value=>send(controlA,value),
  openStream:async revision=>{authorizations++;const random=new Uint8Array(28);crypto.getRandomValues(random);return {url:'ws://127.0.0.1:__WS_PORT__/api/remote/native/stream',
    token:revision.toString(16).padStart(8,'0')+[...random].map(n=>n.toString(16).padStart(2,'0')).join(''),format:'hevc-v1',expires_in_ms:15000};},
  onFallback:reason=>errors.push({side:'sender',reason})});
sender.update({revision:1,paused:false});receiver.reset({revision:1,paused:false});
controlA.onmessage=event=>sender.handleControl(event.data);
b.ondatachannel=event=>{if(receiver.bindChannel(event.channel))return;controlB=event.channel;controlB.onmessage=e=>receiver.handleControl(e.data);};
try {
  await a.setLocalDescription(await a.createOffer());await until(()=>a.iceGatheringState==='complete','offer');
  await b.setRemoteDescription(a.localDescription);await b.setLocalDescription(await b.createAnswer());await until(()=>b.iceGatheringState==='complete','answer');
  await a.setRemoteDescription(b.localDescription);await until(()=>controlA.readyState==='open'&&controlB?.readyState==='open','control');
  if(!await receiver.probe())throw new Error('Edge WebCodecs HEVC unsupported');
  await until(()=>sender.supported||errors.length,'capability');captureCalls++;await sender.capture(1);
  await until(()=>frames.length>=60||errors.length,'60 decoded frames');if(errors.length)throw new Error(JSON.stringify(errors));
  const cleanFrames=frames.length,cleanSender=sender.stats(),cleanReceiver=receiver.stats();
  const blockStart=Date.now(),busyStart=performance.now();while(performance.now()-busyStart<500){}const blockEnd=Date.now();
  await until(()=>frames.length>=cleanFrames+10||errors.length,'recover from main-thread stall');
  if(errors.length)throw new Error(JSON.stringify(errors));
  sender.update({revision:2,paused:true});receiver.reset({revision:2,paused:true});const beforePause=frames.length;await wait(150);
  if(frames.length!==beforePause)throw new Error('stale frame presented while paused');
  sender.update({revision:3,paused:false});receiver.reset({revision:3,paused:false});captureCalls++;await sender.capture(3);
  await until(()=>frames.filter(frame=>frame.revision===3).length>=10||errors.length,'new revision decode');
  if(errors.length)throw new Error(JSON.stringify(errors));
  if(!sender.ownsCapture||authorizations!==2||frames.some(frame=>frame.width!==1920||frame.height!==1080))throw new Error('worker contract invalid');
  return {result:'passed',syntheticOnly:true,ownsCapture:sender.ownsCapture,authorizations,captureCalls,decoded:frames.length,
    cleanDecoded:cleanFrames,cleanDecodedFps:(cleanFrames-1)*1000/(frames[cleanFrames-1].at-frames[0].at),
    sender:cleanSender,receiver:cleanReceiver,mainThreadBlock:{start:blockStart,end:blockEnd},
    pauseResume:{staleFramesPresented:0,newRevisionFrames:frames.filter(frame=>frame.revision===3).length},errors,controls};
}finally{sender.close();receiver.close();a.close();b.close();}
})()"""

async def run(profile,http):
    async with websockets.serve(native_socket,'127.0.0.1',0) as native:
        ws_port=native.sockets[0].getsockname()[1]
        active=profile/'DevToolsActivePort'
        for _ in range(200):
            if active.exists(): break
            await asyncio.sleep(.1)
        cdp=active.read_text().splitlines()[0]
        targets=json.load(urlopen(f'http://127.0.0.1:{cdp}/json/list',timeout=3))
        page=await Page().connect(next(t for t in targets if t['type']=='page'))
        try:
            await page.call('Runtime.enable')
            await page.call('Page.navigate',{'url':f'http://127.0.0.1:{http.server_port}/'})
            await asyncio.sleep(.2)
            result=await page.js(TEST.replace('__WS_PORT__',str(ws_port)))
            result['browser']=await page.call('Browser.getVersion')
            result['source']='synthetic hardware HEVC fixture replay, deadline-paced at 30 FPS; no live native capture'
            result['exceptions']=page.errors
            block=result['mainThreadBlock']
            result['nativeReadsWhileMainBlocked']=sum(block['start']<=read['at']<=block['end'] for read in native_reads)
            assert result['nativeReadsWhileMainBlocked']>=5,result
            assert not page.errors,page.errors
            (ROOT/'docs/verification/2026-10-10-remote-hevc-worker-loopback.json').write_text(json.dumps(result,indent=2),encoding='utf-8')
            print(json.dumps({key:result[key] for key in ['result','browser','ownsCapture','authorizations','captureCalls','decoded','cleanDecodedFps','nativeReadsWhileMainBlocked','pauseResume','errors','exceptions']}),flush=True)
        finally:
            await page.call('Browser.close')
            await page.ws.close()

if __name__=='__main__':
    http=ThreadingHTTPServer(('127.0.0.1',0),Handler)
    threading.Thread(target=http.serve_forever,daemon=True).start()
    profile=Path(tempfile.mkdtemp(prefix='xchat-hevc-worker-edge-'))
    edge=Path(os.environ.get('PROGRAMFILES(X86)','C:/Program Files (x86)'))/'Microsoft/Edge/Application/msedge.exe'
    browser=subprocess.Popen([str(edge),'--headless=new','--remote-debugging-port=0',f'--user-data-dir={profile}',
      '--no-first-run','--no-default-browser-check','--autoplay-policy=no-user-gesture-required','about:blank'],
      stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,creationflags=subprocess.CREATE_NO_WINDOW)
    try: asyncio.run(run(profile,http))
    finally:
        http.shutdown()
        if browser.poll() is None: browser.terminate()
        try: browser.wait(timeout=2)
        except subprocess.TimeoutExpired:
            subprocess.run(['taskkill','/F','/T','/PID',str(browser.pid)],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
