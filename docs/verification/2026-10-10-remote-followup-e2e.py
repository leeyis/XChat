"""Isolated native/WebRTC integration; fake audio, real native screen capture.

Requires remote-qa-launch.py state and the existing headless web binary.
Only local test identities/ports are used; no human contact receives a message.
"""
import argparse, asyncio, ast, base64, json, os, sqlite3, subprocess, sys, tempfile, threading, time, traceback, uuid
from pathlib import Path
from urllib.request import urlopen, Request
from urllib.error import HTTPError
from urllib.parse import urlparse, parse_qs
import websockets

parser=argparse.ArgumentParser()
parser.add_argument('--state',required=True)
parser.add_argument('--binary',default='K:/cargo/debug/lanchat-web.exe')
parser.add_argument('--viewer-only',action='store_true')
parser.add_argument('--focus-emulation',action='store_true',help='Diagnostic only: emulate renderer focus; not an acceptance run')
parser.add_argument('--skip-background',action='store_true')
parser.add_argument('--diagnostic-display-awake',action='store_true',help='Diagnostic only: prevent automatic display idle on this test thread; not an acceptance run')
parser.add_argument('--fps',type=int,choices=[30,60],default=30)
parser.add_argument('--require-native-worker',action='store_true')
parser.add_argument('--report',default='2026-10-10-remote-followup-e2e.json')
parser.add_argument('--monitor-deps',type=Path,help='Optional isolated directory containing psutil')
parser.add_argument('--baseline-max-cpu',type=float,default=20,help='Record explicitly when a diagnostic permits ordinary desktop load above the idle baseline')
args=parser.parse_args()
root=Path(__file__).resolve().parents[2]
temp=Path(tempfile.mkdtemp(prefix='xchat-remote-followup-e2e-'))
processes=[];logs=[];qa_pages=[]
report=dict(result='running',stage='waiting for quiet baseline',checks=[],evidence=str(temp))
if os.name=='nt':
    import ctypes
    from ctypes import wintypes
    process_session=wintypes.DWORD()
    session_ok=ctypes.windll.kernel32.ProcessIdToSessionId(os.getpid(),ctypes.byref(process_session))
    report['windows_session']={
        'process_session_id':process_session.value if session_ok else None,
        'console_session_id':ctypes.windll.kernel32.WTSGetActiveConsoleSessionId(),
        'remote_session':bool(ctypes.windll.user32.GetSystemMetrics(0x1000)),
        'primary_screen':{'width':ctypes.windll.user32.GetSystemMetrics(0),'height':ctypes.windll.user32.GetSystemMetrics(1)},
        'scope':'same-machine integration in the current Windows session; not a physical LAN benchmark',
    }
report_path=root/'docs/verification'/Path(args.report).name
if args.monitor_deps:sys.path.insert(0,str(args.monitor_deps))
import psutil

class LoadMonitor:
    def __init__(self):
        self.samples=[];self.stopped=threading.Event();self.previous={};self.last=time.monotonic()
        self.thread=threading.Thread(target=self.collect,daemon=True)
    def collect(self):
        psutil.cpu_percent()
        while not self.stopped.wait(1):
            at=time.monotonic();elapsed=at-self.last;current={};top=[];competing=[]
            # Name enumeration is cheap; querying every process's creation and
            # CPU times was itself expensive on a host with 1,000+ processes.
            for process in psutil.process_iter(['pid','name'],ad_value=None):
                info=process.info;name=(info['name'] or '').lower()
                if name in ('rustc.exe','codebase-memory-mcp.exe'):competing.append({'pid':info['pid'],'name':name})
                if name not in ('rustc.exe','codebase-memory-mcp.exe','python.exe','lanchat.exe','msedgewebview2.exe','chrome.exe','dwm.exe','taskmgr.exe'):continue
                try:times=process.cpu_times()
                except (psutil.NoSuchProcess,psutil.AccessDenied):continue
                key=info['pid'];used=times.user+times.system;current[key]=used
                if key in self.previous:
                    cpu=max(0,(used-self.previous[key])*100/elapsed/(psutil.cpu_count() or 1))
                    if cpu>=1:top.append({'pid':info['pid'],'name':name,'cpu_percent':round(cpu,2)})
            self.samples.append({'at':time.time(),'cpu_percent':psutil.cpu_percent(),'competing':competing,
                'top':sorted(top,key=lambda item:item['cpu_percent'],reverse=True)[:5]})
            self.previous=current;self.last=at
    async def quiet(self):
        for _ in range(40):
            recent=self.samples[-5:]
            if len(recent)==5 and all(not s['competing'] and s['cpu_percent']<args.baseline_max_cpu for s in recent):return recent
            await asyncio.sleep(1)
        raise RuntimeError('No quiet five-second baseline: '+json.dumps(self.samples[-5:]))
    def between(self,start,end):
        samples=[s for s in self.samples if start<=s['at']<=end]
        return {'samples':samples,'mean_cpu_percent':sum(s['cpu_percent'] for s in samples)/len(samples) if samples else None,
            'max_cpu_percent':max((s['cpu_percent'] for s in samples),default=None),
            'compiler_or_indexer_present':any(s['competing'] for s in samples)}
    def close(self):
        self.stopped.set()
        if self.thread.ident is not None:self.thread.join(3)

load_monitor=LoadMonitor()
def checkpoint(stage):
    report['stage']=stage
    encoded=json.dumps(report,ensure_ascii=False,indent=2)
    # Repository writes can wake graph indexers and contaminate the next sample.
    # Publish the repository copy only after the measurement and cleanup finish.
    (temp/'result.json').write_text(encoded,encoding='utf-8')
    print(stage,flush=True)
tree=ast.parse((Path(__file__).with_name('2026-10-10-remote-e2e.py')).read_text(encoding='utf-8'))
exec(compile(ast.Module(body=[n for n in tree.body if isinstance(n,(ast.ClassDef,ast.FunctionDef)) and n.name in ['Page','api','waitfor']],type_ignores=[]),'<qa-common>','exec'))
original_api=api
def api(port,path,data=None):
    try:return original_api(port,path,data)
    except HTTPError as error:
        raise RuntimeError(f'{port} {path}: HTTP {error.code}: {error.read().decode("utf-8",errors="replace")}') from error
TRACK_PC="""window.__qaPCs=[];{const Original=window.RTCPeerConnection;window.RTCPeerConnection=class extends Original{constructor(...args){super(...args);window.__qaPCs.push(this)}};}
window.__qaCanvasStats=new WeakMap();
{const draw=CanvasRenderingContext2D.prototype.drawImage;CanvasRenderingContext2D.prototype.drawImage=function(...args){
const result=draw.apply(this,args);if(globalThis.VideoFrame&&args[0] instanceof VideoFrame){
const record=__qaCanvasStats.get(this.canvas)||{draws:0,paints:0,waiting:false};record.draws++;__qaCanvasStats.set(this.canvas,record);
if(this.canvas.isConnected&&!record.waiting){record.waiting=true;requestAnimationFrame(()=>{record.waiting=false;record.paints++})}}
return result};}
window.__qaFrameSample=()=>{const c=document.querySelector('.remote-video-viewport canvas');
if(c){const s=__qaCanvasStats.get(c)||{};return{frames:s.paints||0,decodedDraws:s.draws||0,time:performance.now(),width:c.width,height:c.height,presentation:'canvas-paint-callbacks'}}
const v=document.querySelector('video');return v?{frames:v.getVideoPlaybackQuality().totalVideoFrames,time:performance.now(),width:v.videoWidth,height:v.videoHeight,presentation:'video-playback-quality'}:null};
"""
PIXEL_AGE="""(async()=>{
const g=__GEOMETRY__,v=document.querySelector('.remote-video-viewport canvas')||document.querySelector('video');
const size=__qaFrameSample(),c=document.createElement('canvas');c.width=size.width;c.height=size.height;
const x=c.getContext('2d',{willReadFrequently:true}),samples=[];let rejected=0;
for(let n=0;n<25;n++){
  if(v instanceof HTMLVideoElement)await new Promise(resolve=>v.requestVideoFrameCallback(resolve));
  else {const prior=__qaFrameSample().frames,deadline=performance.now()+5000;
    while(__qaFrameSample().frames===prior&&performance.now()<deadline)await new Promise(requestAnimationFrame);}
  x.drawImage(v,0,0);const row=(g.origin.y-g.monitor.position.y+400)*c.height/g.screen.height;let bits='';
  for(let i=0;i<48;i++){const col=(g.origin.x-g.monitor.position.x+48+i*16)*c.width/g.screen.width;
    const p=x.getImageData(Math.floor(col),Math.floor(row),1,1).data;bits+=p[0]+p[1]+p[2]>384?'1':'0'}
  const age=((Date.now()>>>0)-parseInt(bits.slice(16),2))>>>0;
  if(parseInt(bits.slice(0,16),2)===0xd52a&&age<5000)samples.push(age);else rejected++;
}
samples.sort((a,b)=>a-b);return{samples,valid:samples.length,rejected,
mean:samples.length?samples.reduce((a,b)=>a+b,0)/samples.length:null,
p95:samples.length?samples[Math.ceil(samples.length*.95)-1]:null};})()"""

def targets(port):return json.load(urlopen(f'http://127.0.0.1:{port}/json/list',timeout=3))
async def new_page(target):
    page=await Page().connect(target);await page.call('Runtime.enable');await page.call('Page.enable');return page
async def stats(page,expression):
    return await page.js('(async()=>{const pc='+expression+';return pc?Array.from((await pc.getStats()).values()):[]})()')
async def native_viewer_scenario(a,b,native,native_id,pages,checks):
    checkpoint('testing native viewer lifecycle with reversed roles')
    await b.call('Page.bringToFront')
    await b.js("(()=>{__remote.dismiss();const c=document.createElement('canvas');c.width=1280;c.height=720;const x=c.getContext('2d');let n=0;window.__qaSynthetic=setInterval(()=>{x.fillStyle='#203b42';x.fillRect(0,0,1280,720);x.fillStyle='#93e6c6';x.font='40px sans-serif';x.fillText('Remote QA screen '+ ++n,60,100);x.fillRect((n*15)%1180,250,100,180)},33);__remote.setSource(c.captureStream(30));return true})()")
    await b.js("__remote.start("+json.dumps(native_id)+",{mode:'help',screen:{id:'synthetic',name:'QA Screen',width:1280,height:720},note:'Reverse isolated test',control:false,voice:true}).then(()=>true)")
    await a.wait("__remote.state.session?.phase==='waiting'")
    await a.js("__remote.act({type:'accept',screen:null,control:false,voice:true}).then(()=>true)")
    session_id=await a.js('__remote.state.session.id')
    native_view=waitfor(lambda:next((t for t in targets(native['cdp']) if 'view=remote-viewer' in t['url'] and parse_qs(urlparse(t['url']).query).get('session')==[session_id]),None),60)
    nv=await new_page(native_view);pages.append(nv)
    await nv.wait("document.querySelector('video')?.videoWidth===1280",80)
    assert await a.js('__remote.state.delegated && !__remote.media')
    checks.append('native controller opens one independent WebView2 window with its own media owner')
    await nv.js("__TAURI__.window.getCurrentWindow().setFullscreen(true)")
    assert await nv.js("__TAURI__.window.getCurrentWindow().isFullscreen()")
    await nv.js("__TAURI__.window.getCurrentWindow().setFullscreen(false)")
    await nv.wait("!document.querySelector('.remote-viewer.is-fullscreen')",15)
    assert not await nv.js("__TAURI__.window.getCurrentWindow().isFullscreen()")
    checks.append('native fullscreen is allowed by the viewer-specific capability')
    screenshot=await nv.call('Page.captureScreenshot',dict(format='png'))
    (root/'docs/verification/2026-10-10-remote-followup-native-viewer.png').write_bytes(base64.b64decode(screenshot['data']))
    await nv.call('Runtime.evaluate',dict(expression="void __TAURI__.window.getCurrentWindow().close()",returnByValue=True))
    await b.wait("__remote.state.session && !['waiting','connecting','active'].includes(__remote.state.session.phase)",45)
    assert any(t['type']=='page' and '?view=' not in t['url'] for t in targets(native['cdp']))
    checks.append('closing independent native viewer ends only the remote session and keeps main window alive')
    await b.js('clearInterval(__qaSynthetic);true')
async def run():
    native=json.loads(Path(args.state).read_text())
    cleanup=await new_page(next(t for t in targets(native['cdp']) if t['type']=='page' and '?view=' not in t['url']))
    await cleanup.js("(async()=>{clearInterval(window.__qaMotion);document.querySelector('#qa-motion')?.remove();if(window.__remote?.state.session)try{await __remote.act({type:'stop',reason:'ended'})}catch{}return true})()")
    await cleanup.call('Page.reload',dict(ignoreCache=True));await cleanup.ws.close()
    await asyncio.sleep(3)
    load_monitor.thread.start()
    report['quiet_baseline']=await load_monitor.quiet()
    report['baseline_max_cpu']=args.baseline_max_cpu
    report['diagnostic_desktop_load']=args.baseline_max_cpu>20
    checkpoint('starting isolated peers')
    report['diagnostic_background_flags']=native.get('diagnostic_background_flags',False)
    with sqlite3.connect(Path(native['db'])/'xchat.db') as db:native_id=db.execute("SELECT value FROM settings WHERE key='user_id'").fetchone()[0]
    peer_id=str(uuid.uuid4());folder=temp/'peer';folder.mkdir()
    with sqlite3.connect(folder/'xchat.db') as db:
        db.execute('CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL)')
        db.executemany('INSERT INTO settings VALUES(?,?)',[('user_id',peer_id),('username','Remote QA Browser'),('username_source','custom'),('download_path',str(folder/'downloads')),('network.discovery.settings.v1',json.dumps(dict(local_discovery=False,vpn_discovery=False,interface_overrides={})))])
    log=(temp/'peer.log').open('w',encoding='utf-8');logs.append(log)
    processes.append(subprocess.Popen([str(Path(args.binary).resolve()),'--port','18931','--db-path',str(folder)],stdout=log,stderr=log,creationflags=subprocess.CREATE_NO_WINDOW))
    waitfor(lambda:api(18931,'/api/health').get('http_ready'))
    for port,other,identity in [(18931,native['port'],native_id),(native['port'],18931,peer_id)]:
        api(port,'/api/add_custom_peer',dict(peer=f'127.0.0.1:{other}',expected_device_id=identity))
    for port,identity in [(18931,native_id),(native['port'],peer_id)]:
        waitfor(lambda:any(peer['id']==identity for peer in api(port,'/api/get_peers')),30)
        api(port,f'/api/peers/{identity}/refresh',{})
    script=temp/'vite.mjs'
    script.write_text('import {createServer} from '+json.dumps((root/'node_modules/vite/dist/node/index.js').as_uri())+';import config from '+json.dumps((root/'vite.config.js').as_uri())+';const server=await createServer({...config,...'+json.dumps(dict(configFile=False,cacheDir=str(temp/'vite-cache'),server=dict(host='127.0.0.1',port=18913,strictPort=True,proxy={'/api':dict(target='http://127.0.0.1:18931',changeOrigin=False),'/ws':dict(target='ws://127.0.0.1:18931',ws=True,changeOrigin=False)})))+'});await server.listen();',encoding='utf-8')
    log=(temp/'vite.log').open('w',encoding='utf-8');logs.append(log)
    processes.append(subprocess.Popen(['node',str(script)],cwd=root,stdout=log,stderr=log,creationflags=subprocess.CREATE_NO_WINDOW))
    waitfor(lambda:urlopen('http://127.0.0.1:18913/',timeout=2).status==200)
    profile=temp/'chrome';profile.mkdir()
    processes.append(subprocess.Popen([r'C:/Program Files/Google/Chrome/Application/chrome.exe','--headless=new','--no-first-run','--no-default-browser-check','--disable-extensions','--disable-popup-blocking','--autoplay-policy=no-user-gesture-required','--use-fake-ui-for-media-stream','--use-fake-device-for-media-stream','--remote-debugging-port=0','--user-data-dir='+str(profile),'about:blank'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,creationflags=subprocess.CREATE_NO_WINDOW))
    waitfor(lambda:(profile/'DevToolsActivePort').exists());cdp=int((profile/'DevToolsActivePort').read_text().splitlines()[0])
    a=await new_page(next(t for t in targets(native['cdp']) if t['type']=='page' and '?view=' not in t['url']))
    b=await new_page(next(t for t in targets(cdp) if t['type']=='page'));pages=qa_pages;pages.extend([a,b]);checks=report['checks']
    await a.call('Page.reload',dict(ignoreCache=True))
    if args.focus_emulation:
        await a.call('Emulation.setFocusEmulationEnabled',dict(enabled=True))
        report['diagnostic_focus_emulation']=True
    await b.call('Page.navigate',dict(url='http://127.0.0.1:18913/'))
    for page in pages:
        # Import the URL actually loaded by the application. Vite can retain an
        # HMR timestamp in an importer even after a page reload; importing an
        # unversioned singleton would create a second remote client/owner.
        await page.wait("performance.getEntriesByType('resource').some(e=>new URL(e.name).pathname==='/src/remote-client.js')")
        await page.js("(async()=>{window.__qaModule=path=>performance.getEntriesByType('resource').filter(e=>new URL(e.name).pathname===path).at(-1)?.name||path;window.__remote=(await import(__qaModule('/src/remote-client.js'))).remoteClient;await __remote.boot();return __remote.state.ready})()")
        await page.js("(async()=>{window.__qaMediaErrors=[];const {RemoteMedia}=await import(__qaModule('/src/remote-media.js'));if(!RemoteMedia.prototype.__qaFailCapture){const fail=RemoteMedia.prototype.fail;RemoteMedia.prototype.fail=function(error){window.__qaMediaErrors.push(String(error.stack||error));return fail.call(this,error)};RemoteMedia.prototype.__qaFailCapture=true}return true})()")
        await page.js("""(async()=>{const {RemoteMedia}=await import(__qaModule('/src/remote-media.js'));
          if(!RemoteMedia.prototype.__qaCaptureDiagnostics){const capture=RemoteMedia.prototype.captureLoop;
            RemoteMedia.prototype.captureLoop=function(){const reader=this.frameReader,read=reader.read.bind(reader);
              const counts=window.__qaCapture={requests:0,completed:0,changed:0,unchanged:0,errors:0};
              reader.read=async revision=>{counts.requests++;try{const value=await read(revision),raw=value.bytes;
                const bytes=raw instanceof Uint8Array?raw:new Uint8Array(raw),v=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);
                counts.completed++;const unchanged=(v.getUint32(36,true)&1)!==0;counts[unchanged?'unchanged':'changed']++;
                Object.assign(counts,{lastSequence:v.getBigUint64(16,true).toString(),backend:bytes[7],captureMs:v.getUint32(24,true)/1000,lastCompletedAt:performance.now()});
                return value;}catch(error){counts.errors++;counts.lastError=String(error);throw error;}};
              return capture.call(this);};RemoteMedia.prototype.__qaCaptureDiagnostics=true;}return true})()""")
    await a.js("(async()=>{window.__qaViewerStates=[];await __TAURI__.event.listen('xchat:remote-viewer-state',event=>{__qaViewerStates.push(event.payload);if(__qaViewerStates.length>25)__qaViewerStates.shift()});return true})()")
    await a.js("clearInterval(window.__qaMotion);document.querySelectorAll('#qa-motion').forEach(node=>node.remove());true")
    await a.js("(async()=>{if(__remote.state.session&&['waiting','connecting','active'].includes(__remote.state.session.phase))await __remote.act({type:'stop',reason:'ended'});__remote.dismiss();const screens=await __remote.screens(),monitor=await __TAURI__.window.currentMonitor();window.__qaScreen=screens.find(s=>s.name===monitor?.name)||screens.find(s=>s.width===monitor?.size.width&&s.height===monitor?.size.height)||screens[0];return __qaScreen})()")
    if args.viewer_only:
        await native_viewer_scenario(a,b,native,native_id,pages,checks)
        report.update(result='passed',mode='viewer-only',exceptions=[page.errors for page in pages])
        checkpoint('passed native viewer lifecycle')
        for page in pages:
            try:await page.ws.close()
            except Exception:pass
        return
    # Start the visible source before waiting for its first captured frame. A
    # quiescent/occluded desktop supplies no useful animated benchmark workload.
    fixture_monitor=await a.js('__TAURI__.window.currentMonitor()')
    report['source_screen']=await a.js('__qaScreen')
    report['fixture_monitor']=fixture_monitor
    report['fixture_workload']='static desktop with one moving 300x240 region and a 48-bit timestamp barcode; actual source callbacks in motion.log'
    fixture_log=(temp/'motion.log').open('w',encoding='utf-8');logs.append(fixture_log)
    fixture_env={**os.environ,'XCHAT_QA_MONITOR':json.dumps(fixture_monitor),'XCHAT_QA_FPS':str(args.fps)}
    fixture=subprocess.Popen([os.sys.executable,str(Path(__file__).with_name('2026-10-10-remote-motion-fixture.py'))],env=fixture_env,stdin=subprocess.PIPE,stdout=fixture_log,stderr=fixture_log,text=True,creationflags=subprocess.CREATE_NO_WINDOW)
    processes.append(fixture)
    await a.js("(async()=>{await __remote.start("+json.dumps(peer_id)+",{mode:'help',screen:__qaScreen,note:'Isolated performance test',control:false,voice:false});return true})()")
    await b.wait("__remote.state.session?.phase==='waiting'")
    await b.js('__remote.reserveViewer();true')
    popup=waitfor(lambda:next((t for t in targets(cdp) if t['type']=='page' and t['url']=='about:blank'),None))
    viewer=await new_page(popup);pages.append(viewer)
    await viewer.call('Page.addScriptToEvaluateOnNewDocument',dict(source=TRACK_PC))
    await b.js("__remote.act({type:'accept',screen:null,control:false,voice:false}).then(()=>true)")
    checkpoint('waiting for native capture in independent browser viewer')
    await asyncio.gather(a.wait("__remote.state.session?.phase==='active'",80),viewer.wait("window.__qaFrameSample?.()?.width>0",80))
    assert await a.js("!document.querySelector('.remote-stage video') && !document.querySelector('.remote-video-viewport video')")
    assert await b.js("__remote.state.delegated && !__remote.media")
    assert not await viewer.js("!!document.querySelector('.ra-chat-panel')")
    checks.append('host has no recursive preview; controller main has no media owner; separate viewer displays frames without chat sidebar')
    await a.js("__remote.act({type:'quality',quality:{preset:'clear',fps:"+str(args.fps)+",reduced_color:false}}).then(()=>true)")
    # A visible test workload exercises changing native pixels. This changes only
    # the isolated QA renderer and is removed before the second scenario.
    await a.call('Page.bringToFront')
    await a.js("(()=>{const e=document.createElement('canvas');e.id='qa-motion';e.width=1000;e.height=480;e.style='position:fixed;left:100px;top:150px;width:1000px;height:480px;z-index:99999';document.body.append(e);const c=e.getContext('2d');let n=0;window.__qaMotion=setInterval(()=>{c.fillStyle='#203039';c.fillRect(0,0,1000,480);c.fillStyle='#6ae2b8';c.font='32px sans-serif';c.fillText('XChat native capture measurement '+(++n),40,80);c.fillRect((n*12)%930,200,70,120);const bits=0xd52a.toString(2).padStart(16,'0')+(Date.now()>>>0).toString(2).padStart(32,'0');[...bits].forEach((bit,i)=>{c.fillStyle=bit==='1'?'white':'black';c.fillRect(40+i*16,380,16,40)})},33);return true})()")
    try:
        await a.wait('__remote.media?.hevc?.sender?.ready || __remote.media?.hevc?.sender?.failed',20)
        report['hevc_negotiation']='active' if await a.js('__remote.media?.hevc?.sender?.ready===true') else 'fallback_or_unavailable'
    except Exception:
        report['hevc_negotiation']='fallback_or_unavailable'
    if args.require_native_worker and report['hevc_negotiation']!='active':
        raise AssertionError('Native HEVC Worker did not become ready; see first-frame diagnostics')
    await asyncio.sleep(4)
    checkpoint('measuring native worker frame delivery')
    measurement_start=time.time()
    before=await viewer.js('__qaFrameSample()')
    await asyncio.sleep(12)
    after=await viewer.js('__qaFrameSample()')
    measured_fps=(after['frames']-before['frames'])*1000/(after['time']-before['time'])
    video=dict(before=before,after=after,requested_fps=args.fps,decoded_fps=measured_fps,sender_metrics=await a.js('__remote.state.metrics'),receiver_metrics=await b.js('__remote.state.metrics'),sender_rtp=await stats(a,'__remote.media?.pc'),receiver_rtp=await stats(viewer,'window.__qaPCs?.[0]'))
    report['video']=video
    video['system_load']=load_monitor.between(measurement_start,time.time())
    assert not video['system_load']['compiler_or_indexer_present'],video['system_load']
    video['native_hevc_state']=await a.js("({supported:__remote.media?.hevc?.sender?.supported,ready:__remote.media?.hevc?.sender?.ready,failed:__remote.media?.hevc?.sender?.failed,fallbackReason:__remote.media?.hevc?.fallbackReason,stats:__remote.media?.hevc?.sender?.stats()})")
    video['rgba_capture']=await a.js('window.__qaCapture')
    environment="({userAgent:navigator.userAgent,secureContext:isSecureContext,visibility:document.visibilityState,sendCodecs:RTCRtpSender.getCapabilities('video').codecs,receiveCodecs:RTCRtpReceiver.getCapabilities('video').codecs})"
    video['sender_environment']=await a.js(environment)
    video['receiver_environment']=await viewer.js(environment)
    checkpoint('collected native screen-only video metrics')
    if args.require_native_worker:
        assert report['hevc_negotiation']=='active',report['hevc_negotiation']
        assert video['sender_metrics'].get('frameTransport')=='worker-websocket',video['sender_metrics']
    assert after['frames']>before['frames'],dict(before=before,after=after,metrics=video['sender_metrics'])
    assert after['width']==1920 and after['height']==1080,after
    checks.append('actual decoded frames and RTP codec/hardware/network statistics collected over 12 seconds; no FPS target inferred from timer')
    # Decode a high-contrast timestamp written into the source pixels. This is
    # same-machine source-draw to decoded-image age, not input or display-scanout latency.
    try:
        geometry=await a.js("(async()=>({origin:await __TAURI__.window.getCurrentWindow().innerPosition(),monitor:await __TAURI__.window.currentMonitor(),screen:__qaScreen,dpr:devicePixelRatio}))()")
        geometry.update(origin=fixture_monitor['position'],dpr=1,fixture=True)
        video['pixel_age_ms']=await viewer.js(PIXEL_AGE.replace('__GEOMETRY__',json.dumps(geometry)))
    except Exception as error:
        video['pixel_age_ms']={'unavailable':str(error)}
    checkpoint('testing native capture while minimized without a debugger connection')
    native_window=None
    try:
        if args.skip_background:raise RuntimeError('Skipped for this diagnostic run')
        native_window=await a.call('Browser.getWindowForTarget')
        await a.call('Browser.setWindowBounds',dict(windowId=native_window['windowId'],bounds=dict(windowState='minimized')))
        await asyncio.sleep(1)
        hidden=await a.js("(async()=>({minimized:await __TAURI__.window.getCurrentWindow().isMinimized(),visibility:document.visibilityState}))()")
        if not hidden['minimized']:
            await a.js('__TAURI__.window.getCurrentWindow().minimize()')
            await asyncio.sleep(1)
            hidden=await a.js("(async()=>({minimized:await __TAURI__.window.getCurrentWindow().isMinimized(),visibility:document.visibilityState}))()")
        if not hidden['minimized']:
            raise RuntimeError('Native window did not confirm its minimized state')
        bg_before=await viewer.js('__qaFrameSample()')
        background_start=time.time()
        await a.ws.close()
        await asyncio.sleep(12)
        bg_after=await viewer.js('__qaFrameSample()')
        a=await new_page(next(t for t in targets(native['cdp']) if t['type']=='page' and '?view=' not in t['url']));pages.append(a)
        video['background']=dict(**hidden,before=bg_before,after=bg_after,decoded_fps=(bg_after['frames']-bg_before['frames'])*1000/(bg_after['time']-bg_before['time']),sender_debugger_attached=False)
        video['background']['system_load']=load_monitor.between(background_start,time.time())
        assert not video['background']['system_load']['compiler_or_indexer_present'],video['background']['system_load']
        await a.call('Browser.setWindowBounds',dict(windowId=native_window['windowId'],bounds=dict(windowState='normal')))
    except Exception as error:
        video['background']={'unavailable':str(error)}
        try:
            if native_window:await a.call('Browser.setWindowBounds',dict(windowId=native_window['windowId'],bounds=dict(windowState='normal')))
        except Exception:pass
    checkpoint('testing voice and input channel recovery')
    await a.js("__remote.act({type:'voice_invite'}).then(()=>true)")
    await b.wait("__remote.state.session.voice.stage==='ringing'")
    await b.js("__remote.act({type:'voice_answer',id:__remote.state.session.voice.id,accepted:true}).then(()=>true)")
    await asyncio.gather(a.wait('__remote.state.microphone===true'),b.wait('__remote.state.microphone===true'))
    checks.append('fake microphone voice state reaches both main windows through viewer relay')
    await a.js('window.__qaMedia=__remote.media;__remote.media.channel.close();true')
    await a.wait("__qaMedia===__remote.media && __remote.media.channel?.readyState==='open' && !__remote.state.inputError",25)
    assert await a.js("__remote.state.session.phase==='active' && __remote.state.microphone && __remote.media.inputSuspended")
    # Canvas and RTP video have independent counters. A fallback must not be
    # required to catch up to all the frames previously displayed by HEVC.
    video['input_channel_recovery']=await viewer.js("""(async()=>{
      let before=__qaFrameSample();const deadline=performance.now()+15000;
      while(performance.now()<deadline){
        await new Promise(resolve=>setTimeout(resolve,100));const after=__qaFrameSample();
        if(!after||!after.width){before=after;continue}
        if(!before||after.presentation!==before.presentation||after.frames<before.frames){before=after;continue}
        if(after.frames>before.frames+3)return{before,after};
      }
      throw Error('No advancing video frames after input channel recovery');
    })()""")
    video['input_channel_recovery']['sender_metrics']=await a.js('__remote.state.metrics')
    if args.require_native_worker:
        assert await a.js('__remote.media?.hevc?.sender?.ready && !__remote.media?.hevc?.sender?.failed'),video['input_channel_recovery']
    checks.append('closing input SCTP channel recreates it without ending screen/voice or restoring control permission')
    await a.js("clearInterval(__qaMotion);document.querySelector('#qa-motion')?.remove();__remote.act({type:'pause',paused:true}).then(()=>true)")
    await viewer.wait("document.body.innerText.includes('暂停')")
    await asyncio.sleep(.5)
    paused_frames=await viewer.js('__qaFrameSample().frames')
    await asyncio.sleep(.5)
    assert await viewer.js('__qaFrameSample().frames')==paused_frames
    await a.js("__remote.act({type:'pause',paused:false}).then(()=>true)")
    await viewer.wait('__qaFrameSample()?.frames>'+str(paused_frames+3),20)
    if args.require_native_worker:
        await a.wait('__remote.media?.hevc?.sender?.ready && __remote.state.metrics.frameTransport===\'worker-websocket\'',20)
    checks.append('pause stops presenting old frames; resume presents new frames and releases control')
    if fixture.poll() is None:fixture.stdin.write('stop\n');fixture.stdin.flush()
    await a.js("__remote.act({type:'stop',reason:'ended'}).then(()=>true)")
    await b.wait("__remote.state.session?.phase==='ended'")
    await native_viewer_scenario(a,b,native,native_id,pages,checks)
    report['performance_acceptance']={
        'status':'not_certified',
        'reference_decoded_fps':32,
        'local_sample_meets_reference_fps':measured_fps>=32,
        'reference_frame_latency_ms':60,
        'native_encoder_hardware_proven':video['sender_metrics'].get('hardwareEncoder') is True,
        'physical_lan_and_macos_hardware_decoder_verified':False,
        'reason':'Functional test success and same-machine timing do not establish the requested cross-platform minimum',
    }
    result=dict(result='passed',checks=checks,video=video,exceptions=[p.errors for p in pages],evidence=str(temp),limitations=['Windows and Chromium on one physical machine; loopback network is not a LAN latency measurement','Audio sources are synthetic; no physical microphone or macOS runtime validation','FPS is for this 12-second workload only; compare against target explicitly'])
    report.update(result)
    checkpoint('passed')
    print(json.dumps(dict(checks=checks,decoded_fps=measured_fps,pixel_age_ms=video['pixel_age_ms'],sender_metrics=video['sender_metrics'],exceptions=result['exceptions']),ensure_ascii=False),flush=True)
    for page in pages:
        try:await page.ws.close()
        except Exception:pass
async def checked_run():
    try:await run()
    except Exception:
        report['diagnostics']=[]
        for page in qa_pages:
            try:report['diagnostics'].append(await page.js("({url:location.href,body:document.body.innerText.slice(-3500),state:window.__remote?.state,mediaErrors:window.__qaMediaErrors,viewerStates:window.__qaViewerStates,hevc:window.__remote?.media?.hevc?{supported:__remote.media.hevc.sender?.supported,ready:__remote.media.hevc.sender?.ready,failed:__remote.media.hevc.sender?.failed,ownsCapture:__remote.media.hevc.sender?.ownsCapture,fallbackReason:__remote.media.hevc.fallbackReason,stats:__remote.media.hevc.sender?.stats(),receiving:__remote.media.hevc.receiving,detached:__remote.media.hevc.detached}:null,localTracks:window.__remote?.media?.localScreen?.getTracks().map(t=>({kind:t.kind,state:t.readyState,enabled:t.enabled,muted:t.muted})),canvas:window.__remote?.media?.canvas?{width:__remote.media.canvas.width,height:__remote.media.canvas.height}:null,pcStates:window.__qaPCs?.map(p=>({state:p.connectionState,ice:p.iceConnectionState,signal:p.signalingState,local:p.localDescription?.type,remote:p.remoteDescription?.type}))})"))
            except Exception as error:report['diagnostics'].append(dict(unavailable=str(error)))
        raise
execution_state=None
try:
    if args.diagnostic_display_awake:
        import ctypes
        set_execution_state=ctypes.WinDLL('kernel32',use_last_error=True).SetThreadExecutionState
        set_execution_state.argtypes=[ctypes.c_uint32];set_execution_state.restype=ctypes.c_uint32
        execution_state=set_execution_state(0x80000003)
        if not execution_state:raise OSError(ctypes.get_last_error(),'SetThreadExecutionState failed')
        report['diagnostic_display_awake']=True
    asyncio.run(checked_run())
except Exception:
    report['result']='failed';report['error']=traceback.format_exc()
    checkpoint('failed: '+report['stage'])
    raise
finally:
    load_monitor.close()
    for process in reversed(processes):
        if process.poll() is None:subprocess.run(['rtk','proxy','taskkill','/PID',str(process.pid),'/T','/F'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    for log in logs:log.close()
    if execution_state:set_execution_state(execution_state)
    report['load_monitor']=load_monitor.samples
    report_path.write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
    print('evidence',temp,flush=True)
