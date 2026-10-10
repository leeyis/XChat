"""Isolated Edge RTCDataChannel -> DedicatedWorker feature/roundtrip probe."""
import asyncio
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.request import urlopen

ROOT = Path(__file__).resolve().parents[2]
common = (ROOT / 'docs/verification/2026-10-10-remote-voice-loopback.py').read_text(encoding='utf-8')
namespace = {'__file__': str(ROOT / 'docs/verification/2026-10-10-remote-voice-loopback.py')}
exec(common[:common.index('TEST = r')], namespace)
Page = namespace['Page']

WORKER = r"""
self.onmessage = ({data}) => {
  const channel = data.channel;
  channel.binaryType = 'arraybuffer';
  channel.onmessage = event => {
    const value = new Uint8Array(event.data);
    self.postMessage({type:'received',length:value.length,first:value[0],last:value.at(-1),
      exposed:typeof RTCDataChannel,readyState:channel.readyState,label:channel.label});
  };
  channel.onopen = () => {const bytes=new Uint8Array(1200);bytes[0]=42;bytes[1199]=73;channel.send(bytes);};
  channel.onerror = () => self.postMessage({type:'error'});
  self.postMessage({type:'transferred',readyState:channel.readyState,label:channel.label,exposed:typeof RTCDataChannel});
};
"""

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        payload = WORKER.encode() if self.path == '/worker.js' else b'<!doctype html><title>RTC worker transfer probe</title>'
        self.send_response(200)
        self.send_header('Content-Type', 'text/javascript' if self.path == '/worker.js' else 'text/html')
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)
    def log_message(self, *_): pass

TEST = r"""(async()=>{
const worker=new Worker('/worker.js'),a=new RTCPeerConnection({iceServers:[]}),b=new RTCPeerConnection({iceServers:[]});
const events=[],errors=[];worker.onmessage=event=>events.push(event.data);worker.onerror=event=>errors.push(event.message);
const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const until=async(fn,label)=>{for(let i=0;i<200;i++){if(fn())return;await wait(25);}throw new Error(label+JSON.stringify({events,errors}));};
try {
  // The spec's transferability flag expires at the next task: no await here.
  const channel=a.createDataChannel('probe',{ordered:false,maxRetransmits:0});
  worker.postMessage({channel},[channel]);
  const originalStateAfterTransfer=channel.readyState;
  b.ondatachannel=event=>{event.channel.binaryType='arraybuffer';event.channel.onmessage=e=>event.channel.send(e.data);};
  await a.setLocalDescription(await a.createOffer());await until(()=>a.iceGatheringState==='complete','offer ICE');
  await b.setRemoteDescription(a.localDescription);await b.setLocalDescription(await b.createAnswer());await until(()=>b.iceGatheringState==='complete','answer ICE');
  await a.setRemoteDescription(b.localDescription);await until(()=>events.some(e=>e.type==='received'),'worker binary roundtrip');
  const received=events.find(e=>e.type==='received');
  if(received.length!==1200||received.first!==42||received.last!==73||errors.length)throw new Error('roundtrip mismatch');
  return {result:'passed',originalStateAfterTransfer,events,errors,connectionState:a.connectionState,maxMessageSize:a.sctp.maxMessageSize};
}finally{worker.terminate();a.close();b.close();}
})()"""

async def run(profile, server):
    active = profile / 'DevToolsActivePort'
    for _ in range(200):
        if active.exists(): break
        await asyncio.sleep(.1)
    cdp = active.read_text().splitlines()[0]
    targets = json.load(urlopen(f'http://127.0.0.1:{cdp}/json/list', timeout=3))
    page = await Page().connect(next(t for t in targets if t['type'] == 'page'))
    try:
        await page.call('Runtime.enable')
        await page.call('Page.navigate', {'url': f'http://127.0.0.1:{server.server_port}/'})
        await asyncio.sleep(.2)
        report = await page.js(TEST)
        report['browser'] = await page.call('Browser.getVersion')
        report['exceptions'] = page.errors
        (ROOT/'docs/verification/2026-10-10-remote-worker-transfer.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
        print(json.dumps(report),flush=True)
    finally:
        await page.call('Browser.close')
        await page.ws.close()

if __name__ == '__main__':
    server = ThreadingHTTPServer(('127.0.0.1',0), Handler)
    threading.Thread(target=server.serve_forever,daemon=True).start()
    profile=Path(tempfile.mkdtemp(prefix='xchat-rtc-worker-edge-'))
    edge=Path(os.environ.get('PROGRAMFILES(X86)','C:/Program Files (x86)'))/'Microsoft/Edge/Application/msedge.exe'
    browser=subprocess.Popen([str(edge),'--headless=new','--remote-debugging-port=0',f'--user-data-dir={profile}',
      '--no-first-run','--no-default-browser-check','about:blank'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,creationflags=subprocess.CREATE_NO_WINDOW)
    try: asyncio.run(run(profile,server))
    finally:
        server.shutdown()
        if browser.poll() is None: browser.terminate()
        try: browser.wait(timeout=2)
        except subprocess.TimeoutExpired:
            subprocess.run(['taskkill','/F','/T','/PID',str(browser.pid)],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
