import argparse,asyncio,base64,json,os,sqlite3,subprocess,tempfile,time,uuid
from pathlib import Path
from urllib.request import urlopen,Request
import websockets
parser=argparse.ArgumentParser();parser.add_argument('--binary',required=True);parser.add_argument('--dev',action='store_true');args=parser.parse_args()
root=Path.cwd();temp=Path(tempfile.mkdtemp(prefix='xchat-remote-e2e-'));processes=[];logs=[]
def api(port,path,data=None):
 request=Request(f'http://127.0.0.1:{port}'+path,data=None if data is None else json.dumps(data).encode(),headers={'Content-Type':'application/json'})
 return json.load(urlopen(request,timeout=25))
def waitfor(fn,seconds=45):
 end=time.time()+seconds
 while time.time()<end:
  try:
   value=fn()
   if value:return value
  except Exception:pass
  time.sleep(.2)
 raise RuntimeError('Timed out waiting: '+str(fn))
class Page:
 async def connect(self,target):
  self.ws=await websockets.connect(target['webSocketDebuggerUrl'],max_size=16*1024*1024);self.seq=0;self.errors=[];self.lock=asyncio.Lock();return self
 async def call(self,method,params=None):
  async with self.lock:
   self.seq+=1;token=self.seq;await self.ws.send(json.dumps(dict(id=token,method=method,params=params or {})))
   while True:
    value=json.loads(await asyncio.wait_for(self.ws.recv(),40))
    if value.get('method')=='Runtime.exceptionThrown':self.errors.append(value['params'])
    if value.get('id')==token:
     if value.get('error'):raise RuntimeError(value['error'])
     return value.get('result',{})
 async def js(self,source):
  value=await self.call('Runtime.evaluate',dict(expression=source,returnByValue=True,awaitPromise=True,userGesture=True))
  if 'exceptionDetails' in value:raise RuntimeError(value['exceptionDetails'])
  return value.get('result',{}).get('value')
 async def wait(self,source,seconds=45):
  end=time.time()+seconds
  while time.time()<end:
   value=await self.js(source)
   if value:return value
   await asyncio.sleep(.25)
  raise AssertionError(dict(condition=source,body=await self.js('document.body.innerText.slice(-2400)'),exceptions=self.errors))
 async def button(self,text,scope='document'):
  print('button',text,flush=True)
  await self.wait(f"(()=>{{const root={scope};return !!root&&[...root.querySelectorAll('button')].some(b=>!b.disabled&&b.textContent.trim()==="+json.dumps(text)+")})()")
  await self.js(f"[...{scope}.querySelectorAll('button')].find(b=>b.textContent.trim()==="+json.dumps(text)+")?.click()")
 async def shot(self,name):
  shot=await self.call('Page.captureScreenshot',dict(format='png'));(temp/name).write_bytes(base64.b64decode(shot['data']))
SYNTHETIC=r'''
window.__qaStreams=[];window.__qaPCs=[];window.__qaAudios=[];
const RealPC=window.RTCPeerConnection;window.RTCPeerConnection=class extends RealPC{constructor(...args){super(...args);window.__qaPCs.push(this)}};
const RealAudio=window.Audio;window.Audio=function(...args){const a=new RealAudio(...args);window.__qaAudios.push(a);return a};window.Audio.prototype=RealAudio.prototype;
const realMic=navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
navigator.mediaDevices.getUserMedia=async options=>{const stream=await realMic(options);window.__qaStreams.push(stream);return stream};
navigator.mediaDevices.getDisplayMedia=async()=>{
 const canvas=document.createElement('canvas');canvas.width=960;canvas.height=540;const ctx=canvas.getContext('2d');let frame=0;
 const draw=()=>{ctx.fillStyle='#183c43';ctx.fillRect(0,0,960,540);ctx.fillStyle='#78ddbd';ctx.font='32px sans-serif';ctx.fillText('XChat synthetic QA screen',50,100);ctx.fillText('Frame '+ ++frame,50,170);ctx.fillRect(50+frame%700,300,60,60)};
 draw();const stream=canvas.captureStream(15),timer=setInterval(draw,70);window.__qaStreams.push(stream);
 const track=stream.getVideoTracks()[0],originalStop=track.stop.bind(track);track.stop=()=>{clearInterval(timer);originalStop()};
 return stream;
};
'''
async def run():
 identities=[str(uuid.uuid4()),str(uuid.uuid4())];ports=[18921,18922];uiports=[18911,18912] if args.dev else ports
 for index,port in enumerate(ports):
  folder=temp/str(port);folder.mkdir();(folder/'downloads').mkdir()
  with sqlite3.connect(folder/'xchat.db') as db:
   db.execute('CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL)')
   db.executemany('INSERT INTO settings VALUES(?,?)',[('user_id',identities[index]),('username','远程验证 '+str(index+1)),('username_source','custom'),('download_path',str(folder/'downloads')),('network.discovery.settings.v1',json.dumps(dict(local_discovery=False,vpn_discovery=False,interface_overrides={})))])
   db.execute('CREATE TABLE users(id TEXT PRIMARY KEY,name TEXT,addr TEXT,last_seen INTEGER,is_offline INTEGER DEFAULT 0,available_memory_mb INTEGER DEFAULT 0,hostname TEXT,mac_address TEXT,remark TEXT,discovery_source TEXT,app_version TEXT)')
   db.execute('INSERT INTO users(id,name,addr,last_seen,is_offline,discovery_source) VALUES(?,?,?,?,0,?)',(identities[1-index],'远程验证 '+str(2-index),f'127.0.0.1:{ports[1-index]}',int(time.time()),'manual'))
  log=(temp/f'{port}.log').open('w',encoding='utf-8');logs.append(log)
  processes.append(subprocess.Popen([str(Path(args.binary).resolve()),'--port',str(port),'--db-path',str(folder)],stdout=log,stderr=log,creationflags=subprocess.CREATE_NO_WINDOW))
  waitfor(lambda:api(port,'/api/health').get('http_ready'))
 for index,port in enumerate(ports):
  api(port,'/api/add_custom_peer',dict(peer=f'127.0.0.1:{ports[1-index]}',expected_device_id=identities[1-index]));api(port,f'/api/peers/{identities[1-index]}/refresh',{})
 if args.dev:
  script=temp/'vite.mjs';script.write_text('import {createServer} from '+json.dumps((root/'node_modules/vite/dist/node/index.js').as_uri())+';\n'+''.join('const s'+str(i)+'=await createServer('+json.dumps(dict(configFile=str(root/'vite.config.js'),server=dict(host='127.0.0.1',port=uiports[i],strictPort=True,proxy={'/api':dict(target=f'http://127.0.0.1:{ports[i]}',changeOrigin=False),'/ws':dict(target=f'ws://127.0.0.1:{ports[i]}',ws=True,changeOrigin=False)})))+');await s'+str(i)+'.listen();\n' for i in range(2)),encoding='utf-8')
  log=(temp/'vite.log').open('w',encoding='utf-8');logs.append(log);processes.append(subprocess.Popen(['node',str(script)],cwd=root,stdout=log,stderr=log,creationflags=subprocess.CREATE_NO_WINDOW))
  for port in uiports:waitfor(lambda: urlopen(f'http://127.0.0.1:{port}/',timeout=2).status==200)
 profile=temp/'chrome';profile.mkdir()
 browser=subprocess.Popen([r'C:/Program Files/Google/Chrome/Application/chrome.exe','--headless=new','--disable-gpu','--disable-extensions','--no-first-run','--no-default-browser-check','--disable-background-timer-throttling','--disable-backgrounding-occluded-windows','--autoplay-policy=no-user-gesture-required','--use-fake-ui-for-media-stream','--use-fake-device-for-media-stream','--remote-debugging-port=0','--user-data-dir='+str(profile),'about:blank'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,creationflags=subprocess.CREATE_NO_WINDOW);processes.append(browser)
 waitfor(lambda:(profile/'DevToolsActivePort').exists());cdp=(profile/'DevToolsActivePort').read_text().splitlines()[0]
 targets=json.load(urlopen(f'http://127.0.0.1:{cdp}/json/list'));first=next(t for t in targets if t['type']=='page')
 second=json.load(urlopen(Request(f'http://127.0.0.1:{cdp}/json/new?about:blank',method='PUT')))
 pages=[await Page().connect(target) for target in [first,second]]
 for index,page in enumerate(pages):
  await page.call('Runtime.enable');await page.call('Page.enable');await page.call('Page.addScriptToEvaluateOnNewDocument',dict(source=SYNTHETIC))
  await page.call('Emulation.setDeviceMetricsOverride',dict(width=1280,height=850,deviceScaleFactor=1,mobile=False))
  await page.call('Page.navigate',dict(url=f'http://127.0.0.1:{uiports[index]}/'))
  await page.wait("!!document.querySelector('.conversation-list') || document.body.innerText.includes('远程验证')")
  await page.js("[...document.querySelectorAll('button')].find(b=>b.innerText.includes("+json.dumps('远程验证 '+str(2-index))+")&&b.closest('.list-pane'))?.click()")
  for _ in range(25):
   if await page.js("!!document.querySelector('[aria-label=\"远程协助\"]')"):break
   await page.js("[...document.querySelectorAll('button')].find(b=>b.innerText.includes("+json.dumps('远程验证 '+str(2-index))+"))?.click()");await asyncio.sleep(.3)
  await page.wait("!!document.querySelector('[aria-label=\"远程协助\"]')")
 a,b=pages;checks=[]
 await a.js("document.querySelector('[aria-label=\"远程协助\"]').click()")
 await a.wait("[...document.querySelectorAll('.remote-mode-card')].some(b=>!b.disabled)")
 await a.js("document.querySelector('.remote-mode-card').click()")
 await a.wait("!!document.querySelector('[role=dialog]')")
 await a.button('选择屏幕',"document.querySelector('[role=dialog]')")
 await a.wait("[...document.querySelectorAll('[role=dialog] button')].some(b=>b.textContent==='发送协助邀请'&&!b.disabled)")
 await a.button('发送协助邀请',"document.querySelector('[role=dialog]')")
 await b.wait("!!document.querySelector('[role=dialog]')")
 assert await a.js('window.__qaPCs.length===0'), 'media before consent'
 checks.append('no peer connection before consent')
 await b.button('接受协助',"document.querySelector('[role=dialog]')")
 await asyncio.gather(a.wait("!!document.querySelector('.remote-stage')",60),b.wait("!!document.querySelector('.remote-stage')",60))
 await b.wait("document.querySelector('.remote-stage video')?.videoWidth>0",30)
 await asyncio.gather(a.wait("window.__qaStreams.some(s=>s.getAudioTracks().some(t=>t.readyState==='live'))"),b.wait("window.__qaStreams.some(s=>s.getAudioTracks().some(t=>t.readyState==='live'))"))
 async def media(page):return await page.js("(async()=>{let r=[];for(const pc of window.__qaPCs){const stats=await pc.getStats();stats.forEach(s=>{if(['inbound-rtp','outbound-rtp'].includes(s.type))r.push({type:s.type,kind:s.kind,bytes:s.bytesReceived??s.bytesSent,frames:s.framesDecoded??s.framesEncoded})})}return r})()")
 await asyncio.sleep(2);am,bm=await media(a),await media(b)
 assert any(r['type']=='inbound-rtp' and r['kind']=='video' and r.get('frames',0)>0 for r in bm),bm
 for metrics in [am,bm]:
  assert any(r['type']=='inbound-rtp' and r['kind']=='audio' and r['bytes']>0 for r in metrics),metrics
  assert any(r['type']=='outbound-rtp' and r['kind']=='audio' and r['bytes']>0 for r in metrics),metrics
 checks+=['real WebRTC screen frames decoded','real bidirectional audio packets','two independent backend identities']
 await b.shot('remote-viewer.png');await a.shot('remote-host.png')
 await a.button('暂停画面');await b.wait("document.body.innerText.includes('共享方已暂停画面')")
 assert await a.js("window.__qaStreams.some(s=>s.getAudioTracks().some(t=>t.readyState==='live'))")
 checks.append('pause keeps microphone and voice')
 await a.button('继续共享');await b.wait("!document.body.innerText.includes('共享方已暂停画面')")
 await a.button('关闭麦克风');await b.wait("document.body.innerText.includes('对方已静音')");checks.append('microphone state synchronized')
 await a.button('开启麦克风');await b.wait("!document.body.innerText.includes('对方已静音')")
 await b.button('关闭扬声器');assert await b.js('window.__qaAudios.at(-1).muted===true');await b.button('开启扬声器');checks.append('speaker mute is independent')
 await b.button('挂断语音');await a.wait("document.body.innerText.includes('发起语音通话')")
 assert await a.js("document.querySelector('.remote-stage video').srcObject.getVideoTracks()[0].readyState==='live'")
 await a.wait("window.__qaStreams.flatMap(s=>s.getAudioTracks()).every(t=>t.readyState==='ended')");checks.append('voice hangup retains screen and releases microphones')
 await a.js("window.__qaOriginalMic=navigator.mediaDevices.getUserMedia;navigator.mediaDevices.getUserMedia=()=>Promise.reject(new DOMException('QA permission denial','NotAllowedError'))")
 await b.button('发起语音通话');await a.wait("document.body.innerText.includes('接通语音')");await a.button('接通语音')
 await a.wait("document.body.innerText.includes('麦克风不可用')")
 await b.wait("document.body.innerText.includes('发起语音通话') && !!document.querySelector('.remote-stage')")
 checks.append('microphone denial ends only voice and keeps screen session')
 await a.js("navigator.mediaDevices.getUserMedia=options=>new Promise(resolve=>window.__qaResolveMic=async()=>resolve(await window.__qaOriginalMic(options)))")
 await b.button('发起语音通话');await a.wait("document.body.innerText.includes('接通语音')");await a.button('接通语音');await a.wait("typeof window.__qaResolveMic==='function'")
 await b.wait("[...document.querySelectorAll('button')].some(b=>b.textContent==='挂断语音'&&!b.disabled)");await b.button('挂断语音');await a.wait("document.body.innerText.includes('发起语音通话')")
 await a.js("window.__qaResolveMic()");await a.wait("window.__qaStreams.flatMap(s=>s.getAudioTracks()).every(t=>t.readyState==='ended')")
 checks.append('late microphone permission after hangup releases the new track')
 await a.js("navigator.mediaDevices.getUserMedia=window.__qaOriginalMic")
 await b.button('发起语音通话');await a.wait("document.body.innerText.includes('接通语音')");await a.button('接通语音');await b.wait("document.body.innerText.includes('双向通话')");checks.append('mid-session voice invitation accepted')
 await b.js("document.querySelector('[aria-label=\"协助消息\"]')&&Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set.call(document.querySelector('[aria-label=\"协助消息\"]'),'协作消息验证')")
 await b.js("document.querySelector('[aria-label=\"协助消息\"]')?.dispatchEvent(new Event('input',{bubbles:true}))")
 await b.button('发送',"document.querySelector('.remote-chat-compose')")
 await a.wait("document.querySelector('.remote-chat-messages')?.textContent.includes('协作消息验证')");checks.append('remote chat uses durable conversation messages')
 await b.button('全屏');await b.wait("!!document.fullscreenElement");await b.button('自动')
 await b.wait("!!document.fullscreenElement.querySelector('[role=dialog]')");await b.button('取消',"document.querySelector('[role=dialog]')")
 await b.js("document.exitFullscreen()");checks.append('quality dialog remains usable inside fullscreen')
 await b.js("document.documentElement.dataset.theme='dark'")
 layouts=[]
 for width in [1280,860,390]:
  await b.call('Emulation.setDeviceMetricsOverride',dict(width=width,height=850,deviceScaleFactor=1,mobile=False));await asyncio.sleep(.15)
  layout=await b.js("(()=>{const e=document.querySelector('.remote-workspace'),r=e.getBoundingClientRect(),v=document.querySelector('.remote-video-viewport').getBoundingClientRect();return{width:innerWidth,left:r.left,right:r.right,bottom:r.bottom,videoHeight:v.height,scroll:document.documentElement.scrollWidth}})()")
  assert layout['right']<=width+1 and layout['left']>=-1 and layout['videoHeight']>100,layout;layouts.append(layout);await b.shot(f'remote-{width}.png')
 await b.button('× 结束协助');await a.wait("document.body.innerText.includes('远程协助已结束')")
 for page in pages:
  await page.wait("window.__qaStreams.every(s=>s.getTracks().every(t=>t.readyState==='ended')) && window.__qaPCs.every(pc=>pc.connectionState==='closed')")
 checks.append('ending releases every created track and peer connection')
 await b.call('Emulation.setDeviceMetricsOverride',dict(width=1280,height=850,deviceScaleFactor=1,mobile=False))
 await a.button('返回选择');await a.js("document.querySelectorAll('.remote-mode-card')[1].click()")
 await a.button('发送控制请求',"document.querySelector('[role=dialog]')")
 await b.wait("document.querySelector('[role=dialog]')?.textContent.includes('请求控制你的电脑')")
 await b.button('选择屏幕',"document.querySelector('[role=dialog]')")
 await b.button('仅允许查看',"document.querySelector('[role=dialog]')")
 await asyncio.gather(a.wait("!!document.querySelector('.remote-stage')",60),b.wait("!!document.querySelector('.remote-stage')",60))
 await a.wait("document.querySelector('.remote-stage video')?.videoWidth>0")
 assert await b.js("document.querySelector('.remote-head').textContent.includes('正在共享我的屏幕')")
 checks.append('request-control direction shares receiver screen after receiver chooses view-only')
 await a.button('× 结束协助');await b.wait("document.body.innerText.includes('远程协助已结束')")
 await a.button('返回选择');await a.js("document.querySelectorAll('.remote-mode-card')[1].click()")
 await a.button('发送控制请求',"document.querySelector('[role=dialog]')")
 await b.wait("!!document.querySelector('[role=dialog]')");await b.button('拒绝',"document.querySelector('[role=dialog]')")
 await a.wait("document.body.innerText.includes('对方暂时没有同意')");checks.append('receiver rejection reaches initiator without media')
 await a.button('返回选择');await a.js("document.querySelectorAll('.remote-mode-card')[1].click()")
 await a.button('发送控制请求',"document.querySelector('[role=dialog]')")
 await b.wait("!!document.querySelector('[role=dialog]')");await a.button('取消请求',"document.querySelector('.remote-request-actions')")
 await b.wait("document.body.innerText.includes('请求已取消') && !document.querySelector('[role=dialog]')")
 checks.append('initiator cancellation dismisses receiver consent')
 for page in pages:
  await page.wait("window.__qaStreams.every(s=>s.getTracks().every(t=>t.readyState==='ended')) && window.__qaPCs.every(pc=>pc.connectionState==='closed')")
 report=dict(result='passed',checks=checks,media={'host':am,'viewer':bm},layouts=layouts,exceptions=[p.errors for p in pages],device_sources='synthetic canvas screen + Chrome fake microphone only; signaling, backend state and WebRTC are real',frontend='Vite current source' if args.dev else 'embedded production assets',evidence_dir=str(temp))
 (temp/'result.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8');print(json.dumps(report,ensure_ascii=False),flush=True)
 assert not any(p.errors for p in pages)
 for page in pages:await page.ws.close()
try:asyncio.run(run())
except Exception as error:
 (temp/'failure.txt').write_text(str(error),encoding='utf-8');raise
finally:
 for process in reversed(processes):
  if process.poll()is None:
   subprocess.run(['taskkill','/PID',str(process.pid),'/T','/F'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 for log in logs:log.close()
 print('evidence',temp,flush=True)
