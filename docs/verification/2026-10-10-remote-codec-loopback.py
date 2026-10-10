"""Isolated real Chromium video negotiation against production codec preferences."""
import asyncio
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.request import urlopen
import websockets

ROOT = Path(__file__).resolve().parents[2]
common = (ROOT / 'docs/verification/2026-10-10-remote-voice-loopback.py').read_text(encoding='utf-8')
namespace = {'__file__': str(ROOT / 'docs/verification/2026-10-10-remote-voice-loopback.py')}
exec(common[:common.index('TEST = r')], namespace)
Page = namespace['Page']

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        payload = ((ROOT / 'frontend/src/remote-capture.js').read_bytes()
                   if self.path == '/remote-capture.js' else b'<!doctype html><title>Codec loopback QA</title>')
        self.send_response(200)
        self.send_header('Content-Type', 'text/javascript' if self.path.endswith('.js') else 'text/html')
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)
    def log_message(self, *_): pass

TEST = r"""(async()=>{
 const {preferRemoteVideoCodecs}=await import('/remote-capture.js');
 const wait=ms=>new Promise(r=>setTimeout(r,ms));
 const until=async(fn,label)=>{for(let i=0;i<600;i++){if(await fn())return;await wait(25);}throw new Error(label);};
 const a=new RTCPeerConnection({iceServers:[]}),b=new RTCPeerConnection({iceServers:[]});
 const canvas=document.createElement('canvas');canvas.width=320;canvas.height=180;document.body.append(canvas);
 const context=canvas.getContext('2d');let frame=0;const timer=setInterval(()=>{context.fillStyle=`hsl(${++frame*11%360} 70% 50%)`;context.fillRect(0,0,320,180);context.fillStyle='white';context.fillRect(frame*7%280,10,40,160);},33);
 const stream=canvas.captureStream(30), track=stream.getVideoTracks()[0], video=document.createElement('video');video.autoplay=true;video.muted=true;document.body.append(video);
 b.ontrack=e=>{video.srcObject=new MediaStream([e.track]);void video.play();};
 const choices=[];
 const prefer=t=>{let error;const send=RTCRtpSender.getCapabilities('video').codecs,recv=RTCRtpReceiver.getCapabilities('video').codecs;
 const mode='__MODE__';
 const key=c=>c.mimeType.toLowerCase()+'|'+c.clockRate+'|'+(c.sdpFmtpLine||'').replace(/level-id=\d+;?/,'');
 const compatible=recv.filter(c=>send.some(s=>key(c)===key(s)));
 const caps=codecs=>({getCapabilities:()=>({codecs})});
 const args=mode==='receiver'?[caps(recv)]:mode==='sender'?[caps(send)]:mode==='compatible'?[caps(compatible)]:[];
 preferRemoteVideoCodecs({setCodecPreferences:codecs=>{choices.push(codecs);try{t.setCodecPreferences(codecs);}catch(e){error=e;throw e;}}},...args);if(error)throw error;};
 try {
   const t=a.addTransceiver('video',{direction:'sendrecv'});prefer(t);await t.sender.replaceTrack(track);
   await a.setLocalDescription(await a.createOffer());await until(()=>a.iceGatheringState==='complete','offer ICE');
   await b.setRemoteDescription(a.localDescription);const u=b.getTransceivers()[0];u.direction='sendrecv';prefer(u);
   await b.setLocalDescription(await b.createAnswer());
   if(!b.localDescription.sdp.match(/m=video [1-9]/))return {result:'rejected-video',offer:a.localDescription.sdp,answer:b.localDescription.sdp,preferences:choices};
   await until(()=>b.iceGatheringState==='complete',`answer ICE ${b.iceGatheringState} ${b.localDescription.sdp}`);
   await a.setRemoteDescription(b.localDescription);await until(()=>a.connectionState==='connected'&&b.connectionState==='connected','connect');
   await until(()=>video.videoWidth===320,'video dimensions');await wait(2500);
   const sender=await a.getStats(),receiver=await b.getStats();const out=[...sender.values()].find(r=>r.type==='outbound-rtp'&&r.kind==='video');const incoming=[...receiver.values()].find(r=>r.type==='inbound-rtp'&&r.kind==='video');
   if(!(incoming?.framesDecoded>=10))throw new Error(JSON.stringify(incoming));
   return {result:'passed',sender_caps:RTCRtpSender.getCapabilities('video').codecs,receiver_caps:RTCRtpReceiver.getCapabilities('video').codecs,
     preferences:choices,codec:sender.get(out.codecId),framesEncoded:out.framesEncoded,framesDecoded:incoming.framesDecoded,
     dimensions:[incoming.frameWidth,incoming.frameHeight],connectionStates:[a.connectionState,b.connectionState]};
 }finally{clearInterval(timer);track.stop();a.close();b.close();video.srcObject=null;}
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
        mode = sys.argv[1] if len(sys.argv) > 1 else 'production'
        report = await page.js(TEST.replace('__MODE__', mode))
        report['mode'] = mode
        report['browser'] = await page.call('Browser.getVersion')
        report['runtime_exceptions'] = page.errors
        assert not page.errors, page.errors
        (ROOT / f'docs/verification/2026-10-10-remote-codec-loopback-{mode}.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print(json.dumps({k: report.get(k) for k in ['result', 'mode', 'codec', 'framesEncoded', 'framesDecoded', 'dimensions']}), flush=True)
    finally:
        await page.call('Browser.close')
        await page.ws.close()

if __name__ == '__main__':
    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    profile = Path(tempfile.mkdtemp(prefix='xchat-codec-loopback-'))
    chrome = Path(os.environ.get('PROGRAMFILES', 'C:/Program Files')) / 'Google/Chrome/Application/chrome.exe'
    browser = subprocess.Popen([str(chrome), '--headless=new', '--remote-debugging-port=0', f'--user-data-dir={profile}',
        '--no-first-run', '--no-default-browser-check', '--autoplay-policy=no-user-gesture-required', 'about:blank'],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=subprocess.CREATE_NO_WINDOW)
    try: asyncio.run(run(profile, browser, server.server_port))
    finally:
        server.shutdown()
        if browser.poll() is None: browser.terminate()
        try: browser.wait(timeout=2)
        except subprocess.TimeoutExpired:
            subprocess.run(['taskkill', '/F', '/T', '/PID', str(browser.pid)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
