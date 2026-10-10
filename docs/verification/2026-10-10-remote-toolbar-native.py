import argparse,asyncio,ast,base64,json,os,sqlite3,subprocess,tempfile,time,uuid
from pathlib import Path
from urllib.request import urlopen,Request
import websockets
parser=argparse.ArgumentParser();parser.add_argument('--state',required=True,help='Isolated Tauri launcher state JSON');parser.add_argument('--binary',required=True,help='Built lanchat-web executable');args=parser.parse_args()
source=Path(__file__).with_name('2026-10-10-remote-e2e.py').read_text(encoding='utf-8')
tree=ast.parse(source)
exec(compile(ast.Module(body=[node for node in tree.body if isinstance(node,(ast.ClassDef,ast.FunctionDef)) and node.name in ['Page','api','waitfor']],type_ignores=[]),'<qa-common>','exec'))
root=Path.cwd();temp=Path(tempfile.mkdtemp(prefix='xchat-toolbar-native-'));processes=[];logs=[]
async def run():
 native=json.loads(Path(args.state).read_text())
 with sqlite3.connect(Path(native['db'])/'xchat.db') as db: native_id=db.execute("SELECT value FROM settings WHERE key='user_id'").fetchone()[0]
 peer_id=str(uuid.uuid4());folder=temp/'web';folder.mkdir()
 with sqlite3.connect(folder/'xchat.db') as db:
  db.execute('CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL)')
  db.executemany('INSERT INTO settings VALUES(?,?)',[('user_id',peer_id),('username','原生远程联调'),('username_source','custom'),('download_path',str(folder/'downloads')),('network.discovery.settings.v1',json.dumps(dict(local_discovery=False,vpn_discovery=False,interface_overrides={})))])
 log=(temp/'web.log').open('w',encoding='utf-8');logs.append(log)
 processes.append(subprocess.Popen([str(Path(args.binary).resolve()),'--port','18931','--db-path',str(folder)],stdout=log,stderr=log,creationflags=subprocess.CREATE_NO_WINDOW))
 waitfor(lambda:api(18931,'/api/health').get('http_ready'))
 for port,other,identity in [(18931,native['port'],native_id),(native['port'],18931,peer_id)]:
  api(port,'/api/add_custom_peer',dict(peer=f'127.0.0.1:{other}',expected_device_id=identity))
 for port,identity in [(18931,native_id),(native['port'],peer_id)]:
  waitfor(lambda:api(port,f'/api/peers/{identity}/refresh',{}),75)
 script=temp/'vite.mjs';script.write_text('import {createServer} from '+json.dumps((root/'node_modules/vite/dist/node/index.js').as_uri())+';const s=await createServer('+json.dumps(dict(configFile=str(root/'vite.config.js'),server=dict(host='127.0.0.1',port=18913,strictPort=True,proxy={'/api':dict(target='http://127.0.0.1:18931',changeOrigin=False),'/ws':dict(target='ws://127.0.0.1:18931',ws=True,changeOrigin=False)})))+');await s.listen();',encoding='utf-8')
 log=(temp/'vite.log').open('w',encoding='utf-8');logs.append(log);processes.append(subprocess.Popen(['node',str(script)],cwd=root,stdout=log,stderr=log,creationflags=subprocess.CREATE_NO_WINDOW))
 waitfor(lambda:urlopen('http://127.0.0.1:18913/',timeout=2).status==200)
 profile=temp/'chrome';profile.mkdir();processes.append(subprocess.Popen([r'C:/Program Files/Google/Chrome/Application/chrome.exe','--headless=new','--disable-gpu','--disable-extensions','--no-first-run','--disable-background-timer-throttling','--autoplay-policy=no-user-gesture-required','--use-fake-ui-for-media-stream','--use-fake-device-for-media-stream','--remote-debugging-port=0','--user-data-dir='+str(profile),'about:blank'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,creationflags=subprocess.CREATE_NO_WINDOW))
 waitfor(lambda:(profile/'DevToolsActivePort').exists());cdp=(profile/'DevToolsActivePort').read_text().splitlines()[0]
 target=next(t for t in json.load(urlopen(f'http://127.0.0.1:{cdp}/json/list')) if t['type']=='page')
 native_target=next(t for t in json.load(urlopen(f'http://127.0.0.1:{native["cdp"]}/json/list')) if t['type']=='page' and 'remote-toolbar' not in t['url'])
 a,b=await Page().connect(native_target),await Page().connect(target);pages=[a,b];checks=[]
 for page in pages:await page.call('Runtime.enable');await page.call('Page.enable')
 await a.js("(async()=>{window.__remote=(await import('/src/remote-client.js')).remoteClient;await __remote.boot();if(__remote.state.session&&['waiting','connecting','active'].includes(__remote.state.session.phase))await __remote.act({type:'stop',reason:'ended'}).catch(()=>{});__remote.dismiss();return true})()")
 await b.call('Emulation.setDeviceMetricsOverride',dict(width=1280,height=850,deviceScaleFactor=1,mobile=False))
 for _ in range(3):
  navigation=await b.call('Page.navigate',dict(url='http://127.0.0.1:18913/'))
  if not navigation.get('errorText'):break
  await asyncio.sleep(1)
 assert not navigation.get('errorText'),navigation
 await b.wait("document.body.innerText.includes('阶段三桌面验证')")
 await a.js("window.dispatchEvent(new CustomEvent('xchat:remote-open',{detail:{peerId:"+json.dumps(peer_id)+"}}))")
 await a.wait("[...document.querySelectorAll('.remote-mode-card')].some(b=>!b.disabled)")
 await a.js("document.querySelector('.remote-mode-card').click()")
 await a.wait("!!document.querySelector('.remote-screen-options input')")
 await a.js("[...document.querySelectorAll('[role=dialog] label')].find(l=>l.textContent.includes('同时允许对方操作'))?.querySelector('input').click()")
 await a.button('发送协助邀请',"document.querySelector('[role=dialog]')")
 try:await b.wait("!!document.querySelector('[role=dialog]')")
 except Exception:
  print('host state',await a.js("document.body.innerText.slice(-2200)"),flush=True);raise
 denied=await a.js("(async()=>{try{await __TAURI__.core.invoke('remote_frame',{actor:__remote.actor,id:__remote.state.session.id,revision:0});return false}catch{return true}})()")
 assert denied;checks.append('native capture denied before peer consent')
 await b.button('接受协助',"document.querySelector('[role=dialog]')")
 await asyncio.gather(a.wait("!!document.querySelector('.remote-stage')",70),b.wait("!!document.querySelector('.remote-stage')",70))
 await a.js("window.__qaFrameOk=0;{const capture=__remote.media.frame;__remote.media.frame=async revision=>{const bytes=await capture(revision);window.__qaFrameOk++;window.__qaFrameBytes=bytes.byteLength;return bytes;};}")
 await a.wait("window.__qaFrameOk>=3 && window.__qaFrameBytes>1000",45)
 await b.wait("document.querySelector('.remote-stage video')?.getVideoPlaybackQuality().totalVideoFrames>5",45)
 await a.wait("!!__remote.state.session.grant")
 info=await b.js("(()=>{const v=document.querySelector('.remote-stage video');return{width:v.videoWidth,height:v.videoHeight,decoded:v.getVideoPlaybackQuality().totalVideoFrames}})()")
 info['native_frames']=await a.js("({count:window.__qaFrameOk,bytes:window.__qaFrameBytes})")
 checks.append('real native xcap frames decoded via WebRTC; no screen pixels persisted')
 await a.js("window.__grant=__remote.state.session.grant")
 valid=await a.js("(async()=>{await __TAURI__.core.invoke('remote_input',{actor:__remote.actor,id:__remote.state.session.id,packet:{grant:__grant,sequence:5000,event:{type:'keep_alive'}}});return true})()")
 assert valid;checks.append('authorized data input validates with no-op keepalive (no OS keys or pointer injected)')
 for expression in ["{actor:'foreign-owner',id:__remote.state.session.id,packet:{grant:__grant,sequence:5001,event:{type:'keep_alive'}}}","{actor:__remote.actor,id:__remote.state.session.id,packet:{grant:'foreign-grant',sequence:5001,event:{type:'keep_alive'}}}","{actor:__remote.actor,id:__remote.state.session.id,packet:{grant:__grant,sequence:5000,event:{type:'keep_alive'}}}"]:
  assert await a.js("(async()=>{try{await __TAURI__.core.invoke('remote_input',"+expression+");return false}catch{return true}})()")
 checks.append('wrong actor, wrong grant and replayed input rejected')
 toolbar_target=next(t for t in json.load(urlopen(f'http://127.0.0.1:{native["cdp"]}/json/list')) if 'remote-toolbar' in t['url']);bar=await Page().connect(toolbar_target);await bar.call('Runtime.enable')
 await bar.wait("!!document.querySelector('.ra-floating-panel')");pages.append(bar)
 async def click(page,selector):
  await page.js("(()=>{const e=[...document.querySelectorAll("+json.dumps(selector)+")].find(e=>e.checkVisibility());if(!e)throw Error('Missing control');e.click();})()")
 async def geometry():
  return await bar.js("(async()=>{const w=__TAURI__.window.getCurrentWindow(),p=await w.outerPosition(),s=await w.scaleFactor(),m=await __TAURI__.window.currentMonitor();return {x:p.x,y:p.y,width:innerWidth,height:innerHeight,scale:s,area:m.workArea,collapsed:document.querySelector('.ra-floating-panel').classList.contains('collapsed'),errors:document.querySelector('[role=alert]')?.textContent}})()")
 await asyncio.sleep(1)
 full=await geometry();assert not full.get('errors'),full
 frame=await a.js('window.__qaFrameOk')
 await click(bar,'.ra-panel-hide')
 await a.wait("document.querySelector('.ra-floating-panel')?.classList.contains('collapsed')")
 await bar.wait("innerWidth<400")
 await a.wait('window.__qaFrameOk>'+str(frame+3))
 assert await a.js("!!__remote.state.session.grant && !!__remote.media.microphone && !__remote.state.session.paused")
 checks.append('native fold synchronizes main view while real frames, grant and voice continue')
 compact=await geometry()
 await bar.js("document.querySelector('.ra-panel-compact .ra-panel-grip').focus()")
 await bar.call('Input.dispatchKeyEvent',dict(type='keyDown',key='ArrowRight',code='ArrowRight',windowsVirtualKeyCode=39))
 await bar.call('Input.dispatchKeyEvent',dict(type='keyUp',key='ArrowRight',code='ArrowRight',windowsVirtualKeyCode=39))
 await asyncio.sleep(.4)
 moved=await geometry();assert abs(moved['x']-compact['x']-8*compact['scale'])<=1,(compact,moved)
 await click(a,'.ra-panel-restore');await bar.wait("innerWidth>400")
 await asyncio.sleep(.3)
 restored=await geometry();assert abs(restored['x']-full['x'])<=1 and abs(restored['y']-full['y'])<=1,(full,restored)
 checks.append('native keyboard move is scaled correctly and expanding restores full position')
 await bar.js("(async()=>{const a=(await __TAURI__.window.currentMonitor()).workArea;await __TAURI__.window.getCurrentWindow().setPosition(new __TAURI__.window.PhysicalPosition(a.position.x+a.size.width-15,a.position.y+a.size.height-15));})()")
 await asyncio.sleep(.8)
 clamped=await geometry();area=clamped['area']
 # innerWidth/innerHeight are rounded CSS pixels, unlike Tauri's physical coordinates.
 assert clamped['x']>=area['position']['x'] and clamped['x']+clamped['width']*clamped['scale']<=area['position']['x']+area['size']['width']+clamped['scale'],clamped
 assert clamped['y']>=area['position']['y'] and clamped['y']+clamped['height']*clamped['scale']<=area['position']['y']+area['size']['height']+clamped['scale'],clamped
 checks.append('native moved window clamps to physical monitor work area')
 for _ in range(3):
  await click(a,'.ra-panel-hide');await click(a,'.ra-panel-restore')
 await asyncio.sleep(.5)
 assert not (await geometry())['collapsed'] and (await geometry())['width']==full['width']
 checks.append('rapid fold/restore ends with current layout and size')
 await bar.js("document.activeElement?.blur()")
 await bar.call('Input.dispatchMouseEvent',dict(type='mouseMoved',x=1,y=1))
 shot=await bar.call('Page.captureScreenshot',dict(format='png'))
 (root/'docs/verification/2026-10-10-remote-toolbar-native.png').write_bytes(base64.b64decode(shot['data']))
 await a.js("document.querySelector('[data-od-id=nav-chat]').click()")
 await a.wait("!document.querySelector('.remote-workspace') && !!document.querySelector('.remote-floating')")
 await click(a,'.remote-floating');await a.wait("!!document.querySelector('.remote-workspace')")
 checks.append('main navigation minimizes and reopens session without stopping media')
 await click(bar,'.ra-panel-hide');await bar.wait("document.querySelector('.ra-floating-panel').classList.contains('collapsed')")
 await click(bar,'.ra-panel-compact [aria-label="收回控制"]')
 await b.wait("document.body.innerText.includes('当前仅查看')")
 assert await a.js("(async()=>{try{await __TAURI__.core.invoke('remote_input',{actor:__remote.actor,id:__remote.state.session.id,packet:{grant:__grant,sequence:2,event:{type:'keep_alive'}}});return false}catch{return true}})()")
 checks.append('compact native toolbar revoke invalidates old grant')
 await click(bar,'.ra-panel-restore');await bar.wait("!document.querySelector('.ra-floating-panel').classList.contains('collapsed')")
 await a.button('允许对方控制');await a.wait("!!document.querySelector('[role=dialog]')")
 await a.button('允许本次控制',"document.querySelector('[role=dialog]')")
 await a.wait("__remote.state.session.grant && __remote.state.session.grant!==window.__grant")
 checks.append('host proactive control offer uses explicit real consent and a fresh grant')
 await bar.button('收回控制')
 await a.wait("!__remote.state.session.grant")
 await b.button('申请控制');await a.wait("document.querySelector('[role=dialog]')?.textContent.includes('允许本次控制')")
 await a.button('保持仅查看',"document.querySelector('[role=dialog]')");await b.wait("[...document.querySelectorAll('button')].some(b=>b.textContent==='申请控制'&&!b.disabled)")
 await b.button('申请控制');await a.wait("!!document.querySelector('[role=dialog]')");await a.button('允许本次控制',"document.querySelector('[role=dialog]')")
 await a.wait("__remote.state.session.grant && __remote.state.session.grant!==window.__grant")
 checks.append('control re-request requires consent and rotates grant')
 await bar.button('暂停共享');await b.wait("document.body.innerText.includes('画面已暂停共享')")
 await asyncio.sleep(2);assert await a.js("__remote.state.session.phase==='active' && !__remote.state.session.grant && !!__remote.media.microphone")
 await bar.button('继续共享');await b.wait("!document.body.innerText.includes('画面已暂停共享')")
 checks.append('toolbar pause revokes control, preserves voice, resumes without stale-frame teardown')
 await click(bar,'.ra-panel-hide');await click(bar,'.ra-panel-compact [aria-label="结束协助"]');await b.wait("document.body.innerText.includes('远程协助已结束')")
 await a.wait("!__remote.media")
 for _ in range(30):
  if not any('remote-toolbar' in t['url'] for t in json.load(urlopen(f'http://127.0.0.1:{native["cdp"]}/json/list'))):break
  await asyncio.sleep(.3)
 else:raise AssertionError('toolbar remained after end')
 checks.append('end tears down media and closes persistent toolbar')
 report=dict(result='passed',checks=checks,video=info,geometry=dict(full=full,compact=compact,moved=moved,restored=restored,clamped=clamped),exceptions=[p.errors for p in pages],evidence_dir=str(temp),limitations='Native screen bytes used only in memory; no real OS key/mouse injection; microphones are fake browser devices')
 (root/'docs/verification/2026-10-10-remote-toolbar-native.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8',newline='\n');(temp/'result.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8');print(json.dumps(report,ensure_ascii=False),flush=True)
 assert not any(p.errors for p in pages)
 for page in pages:await page.ws.close()
try:asyncio.run(run())
except Exception as error:
 (temp/'failure.txt').write_text(str(error),encoding='utf-8');raise
finally:
 for process in reversed(processes):
  if process.poll() is None:subprocess.run(['rtk','proxy','taskkill','/PID',str(process.pid),'/T','/F'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 for log in logs:log.close()
 print('evidence',temp,flush=True)
